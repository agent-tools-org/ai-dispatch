// Auto-selection heuristics for `aid run auto`.
// Scores prompt signals via capability matrix, respects installed CLIs, returns concise reason.
// Exports select_agent() helpers; deps: super::detect_agents, super::RunOpts.

use super::classifier::{self, Complexity, TaskCategory};
use super::{detect_agents, RunOpts};
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use crate::agent::custom::CustomAgentConfig;
use crate::agent::registry::load_custom_agents;
use crate::team::TeamConfig;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::process::Command;

pub(crate) const AGENT_CAPABILITIES: &[(AgentKind, &[(TaskCategory, i32)])] = &[
    (AgentKind::Gemini, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 6),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 2),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Frontend, 2),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Codex, &[
        (TaskCategory::ComplexImpl, 9), (TaskCategory::Refactoring, 8),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 4), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 4), (TaskCategory::Documentation, 3),
    ]),
    (AgentKind::OpenCode, &[
        (TaskCategory::SimpleEdit, 8), (TaskCategory::Documentation, 5),
        (TaskCategory::Testing, 4), (TaskCategory::Debugging, 4),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 4),
    ]),
    (AgentKind::Kilo, &[
        (TaskCategory::SimpleEdit, 7), (TaskCategory::Documentation, 4),
        (TaskCategory::Testing, 3), (TaskCategory::Debugging, 3),
        (TaskCategory::ComplexImpl, 2), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Cursor, &[
        (TaskCategory::Frontend, 9), (TaskCategory::ComplexImpl, 7),
        (TaskCategory::Refactoring, 6), (TaskCategory::Testing, 5),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 4),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Codebuff, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Frontend, 7), (TaskCategory::Testing, 6),
        (TaskCategory::Debugging, 6), (TaskCategory::SimpleEdit, 5),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
];

fn base_score(agent: AgentKind, category: TaskCategory) -> i32 {
    AGENT_CAPABILITIES.iter()
        .find(|(k, _)| *k == agent)
        .and_then(|(_, scores)| scores.iter().find(|(c, _)| *c == category))
        .map(|(_, s)| *s).unwrap_or(1)
}
fn priority(kind: AgentKind) -> i32 {
    match kind {
        AgentKind::Gemini | AgentKind::Kilo => 0,
        AgentKind::OpenCode => 1, AgentKind::Cursor | AgentKind::Codebuff => 2, AgentKind::Codex => 3,
        AgentKind::Custom => 1,
    }
}
fn cost_efficiency(quality_score: f64, avg_cost: f64) -> f64 {
    let normalized_cost = avg_cost.max(0.0);
    quality_score / (1.0 + normalized_cost)
}

fn custom_category_score(config: &CustomAgentConfig, category: TaskCategory) -> i32 {
    let caps = &config.capabilities;
    match category {
        TaskCategory::Research => caps.research,
        TaskCategory::SimpleEdit => caps.simple_edit,
        TaskCategory::ComplexImpl => caps.complex_impl,
        TaskCategory::Frontend => caps.frontend,
        TaskCategory::Debugging => caps.debugging,
        TaskCategory::Testing => caps.testing,
        TaskCategory::Refactoring => caps.refactoring,
        TaskCategory::Documentation => caps.documentation,
    }
}

fn category_strength_key(category: TaskCategory) -> &'static str {
    match category {
        TaskCategory::Research => "research",
        TaskCategory::SimpleEdit => "simple_edit",
        TaskCategory::ComplexImpl => "complex_impl",
        TaskCategory::Frontend => "frontend",
        TaskCategory::Debugging => "debugging",
        TaskCategory::Testing => "testing",
        TaskCategory::Refactoring => "refactoring",
        TaskCategory::Documentation => "documentation",
    }
}

fn custom_strength_bonus(config: &CustomAgentConfig, category: TaskCategory) -> i32 {
    let key = category_strength_key(category);
    if config.strengths.iter().any(|s| s.eq_ignore_ascii_case(key)) {
        5
    } else {
        0
    }
}

fn custom_command_installed(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

const BUILTIN_AGENTS: &[AgentKind] = &[
    AgentKind::Gemini,
    AgentKind::OpenCode,
    AgentKind::Kilo,
    AgentKind::Cursor,
    AgentKind::Codex,
    AgentKind::Codebuff,
];

#[derive(Clone)]
struct Candidate {
    kind: AgentKind,
    quality: i32,
    efficiency: f64,
    is_default: bool,
    priority: i32,
}

struct CandidateContext<'a> {
    profile: &'a classifier::TaskProfile,
    team: Option<&'a TeamConfig>,
    history_map: &'a HashMap<AgentKind, (f64, usize)>,
    avg_cost_map: &'a HashMap<AgentKind, f64>,
    team_default: Option<AgentKind>,
}

fn score_for(ctx: &CandidateContext<'_>, kind: AgentKind) -> i32 {
    let mut s = if let Some(tc) = ctx.team {
        team_override_score(tc, kind.as_str(), ctx.profile.category)
            .unwrap_or_else(|| base_score(kind, ctx.profile.category))
    } else {
        base_score(kind, ctx.profile.category)
    };
    if rate_limit::is_rate_limited(&kind) {
        s -= 10;
    }
    if let Some((rate, count)) = ctx.history_map.get(&kind) {
        if *count >= 5 {
            let bonus = ((*rate - 0.75) * 16.0).round() as i32;
            let bonus = bonus.clamp(-5, 4);
            s += bonus;
        }
    }
    if matches!(ctx.profile.complexity, Complexity::High)
        && matches!(kind, AgentKind::Codex | AgentKind::Cursor)
    {
        s += 2;
    }
    // Boost preferred agents from team (soft preference, not hard filter)
    if let Some(tc) = ctx.team {
        if tc
            .preferred_agents
            .iter()
            .any(|a| a.eq_ignore_ascii_case(kind.as_str()))
        {
            s += 3;
        }
    }
    s
}

fn candidate_for(kind: AgentKind, ctx: &CandidateContext<'_>) -> Candidate {
    let quality = score_for(ctx, kind);
    let avg_cost = ctx.avg_cost_map.get(&kind).copied().unwrap_or(0.0);
    Candidate {
        kind,
        quality,
        efficiency: cost_efficiency(quality as f64, avg_cost),
        is_default: ctx.team_default == Some(kind),
        priority: priority(kind),
    }
}

fn compare_candidates(a: &Candidate, b: &Candidate, budget: bool) -> Ordering {
    let primary = if budget {
        a.efficiency.partial_cmp(&b.efficiency).unwrap_or(Ordering::Equal)
    } else {
        a.quality.cmp(&b.quality)
    };
    let mut ord = primary;
    if ord == Ordering::Equal {
        ord = if budget {
            a.quality.cmp(&b.quality)
        } else {
            a.efficiency
                .partial_cmp(&b.efficiency)
                .unwrap_or(Ordering::Equal)
        };
    }
    if ord == Ordering::Equal {
        ord = a.is_default.cmp(&b.is_default);
    }
    if ord == Ordering::Equal {
        ord = a.priority.cmp(&b.priority);
    }
    ord
}

fn pick_best_candidate(agents: &[AgentKind], ctx: &CandidateContext<'_>, budget: bool) -> Candidate {
    agents
        .iter()
        .map(|&kind| candidate_for(kind, ctx))
        .max_by(|a, b| compare_candidates(a, b, budget))
        .unwrap_or_else(|| candidate_for(AgentKind::Codex, ctx))
}

pub(crate) fn select_agent_with_reason(
    prompt: &str, opts: &RunOpts, store: &Store,
    team: Option<&TeamConfig>,
) -> (String, String) {
    let available = detect_agents();
    select_agent_from(prompt, opts, &available, store, team)
}

fn select_agent_from(
    prompt: &str, opts: &RunOpts, available: &[AgentKind],
    store: &Store,
    team: Option<&TeamConfig>,
) -> (String, String) {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = classifier::count_file_mentions(&normalized);
    let profile = classifier::classify(prompt, file_count, prompt_len);
    let auto_budget = classifier::contains_any(&normalized, classifier::LOW_VALUE_TERMS);
    let budget = opts.budget || auto_budget;
    let history_map: HashMap<AgentKind, (f64, usize)> = store
        .agent_success_rates()
        .unwrap_or_default()
        .into_iter()
        .map(|(kind, rate, count)| (kind, (rate, count)))
        .collect();
    let avg_cost_map: HashMap<AgentKind, f64> = store
        .agent_avg_costs()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let team_default = team.and_then(|t| t.default_agent.as_deref())
        .and_then(AgentKind::parse_str);
    let ctx = CandidateContext {
        profile: &profile,
        team,
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default,
    };
    let primary_candidate = pick_best_candidate(BUILTIN_AGENTS, &ctx, budget);
    let available_candidate = if available.is_empty() {
        pick_best_candidate(BUILTIN_AGENTS, &ctx, budget)
    } else {
        pick_best_candidate(available, &ctx, budget)
    };
    let mut selected_name = available_candidate.kind.as_str().to_string();
    let mut selected_score = available_candidate.quality;
    let mut selected_builtin = Some(available_candidate.kind);
    if let Some((custom_name, custom_score)) = load_custom_agents()
        .into_values()
        .filter(|config| AgentKind::parse_str(&config.id).is_none())
        .filter(|config| custom_command_installed(&config.command))
        .map(|config| {
            let mut score = custom_category_score(&config, profile.category);
            score += custom_strength_bonus(&config, profile.category);
            // Boost preferred custom agents from team
            if let Some(tc) = &team {
                if tc.preferred_agents.iter().any(|a| a.eq_ignore_ascii_case(&config.id)) {
                    score += 3;
                }
            }
            (config.id, score)
        })
        .max_by_key(|(_, score)| *score)
    {
        let custom_priority = priority(AgentKind::Custom);
        let available_priority = priority(available_candidate.kind);
        if (custom_score, custom_priority) > (available_candidate.quality, available_priority) {
            selected_name = custom_name;
            selected_score = custom_score;
            selected_builtin = None;
        }
    }
    let selected_avg_cost = selected_builtin
        .and_then(|kind| avg_cost_map.get(&kind).copied())
        .unwrap_or(0.0);
    let selected_efficiency = cost_efficiency(selected_score as f64, selected_avg_cost);
    let selected_label = if budget && selected_builtin.is_some() {
        format!(
            "{} (score: {}, avg: ${:.2}, efficiency: {:.1})",
            selected_name, selected_score, selected_avg_cost, selected_efficiency
        )
    } else {
        format!("{} (score: {})", selected_name, selected_score)
    };
    let mut reason = format!(
        "{} task ({}) \u{2192} {}",
        profile.category.label(), profile.complexity.label(),
        selected_label,
    );
    if let Some(sel_kind) = selected_builtin {
        if sel_kind != primary_candidate.kind {
            reason.push_str(&format!("; {} unavailable", primary_candidate.kind.as_str()));
        }
    }
    if auto_budget {
        reason.push_str("; auto-budget: low-value task");
    } else if opts.budget {
        reason.push_str("; budget mode");
    }
    if rate_limit::is_rate_limited(&AgentKind::Codex) && selected_name != AgentKind::Codex.as_str() {
        reason.push_str("; codex rate-limited");
    }
    if let Some(sel_kind) = selected_builtin {
        if let Some((rate, count)) = history_map.get(&sel_kind) {
            if *count >= 5 {
                let percent = (*rate * 100.0).round() as i32;
                let success_label = format!("{:.0}%", rate * 100.0);
                reason.push_str(&format!("; history: {}% success (success: {})", percent, success_label));
            }
        }
    }
    if let Ok(similar_tasks) = store.find_similar_tasks(prompt, 5) {
        let mut stats: HashMap<AgentKind, (usize, usize)> = HashMap::new();
        for (_, agent, status) in similar_tasks {
            let entry = stats.entry(agent).or_insert((0, 0));
            entry.1 += 1;
            if matches!(status, TaskStatus::Done | TaskStatus::Merged) {
                entry.0 += 1;
            }
        }
        if let Some((&agent, &(successes, total))) = stats.iter().max_by(|a, b| {
            a.1 .0.cmp(&b.1 .0).then(a.1 .1.cmp(&b.1 .1))
        }) {
            if successes >= 3 {
                reason.push_str(&format!(
                    "; similar tasks: {} {}/{} success",
                    agent.as_str(),
                    successes,
                    total,
                ));
            }
        }
    }
    (selected_name, reason)
}

pub(crate) fn budget_ranked_agents(
    prompt: &str,
    _opts: &RunOpts,
    store: &Store,
    team: Option<&TeamConfig>,
) -> Vec<AgentKind> {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = classifier::count_file_mentions(&normalized);
    let profile = classifier::classify(prompt, file_count, prompt_len);
    let history_map: HashMap<AgentKind, (f64, usize)> = store
        .agent_success_rates()
        .unwrap_or_default()
        .into_iter()
        .map(|(kind, rate, count)| (kind, (rate, count)))
        .collect();
    let avg_cost_map: HashMap<AgentKind, f64> = store
        .agent_avg_costs()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let team_default = team.and_then(|t| t.default_agent.as_deref())
        .and_then(AgentKind::parse_str);
    let ctx = CandidateContext {
        profile: &profile,
        team,
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default,
    };
    let mut candidates: Vec<Candidate> = BUILTIN_AGENTS
        .iter()
        .map(|&kind| candidate_for(kind, &ctx))
        .collect();
    candidates.sort_by(|a, b| compare_candidates(a, b, true).reverse());
    candidates.into_iter().map(|c| c.kind).collect()
}

pub(crate) fn recommend_model(
    agent: &AgentKind, complexity: &Complexity, budget: bool,
) -> Option<&'static str> {
    use crate::cmd::config::{budget_model, models_for_agent};
    if budget { return budget_model(agent); }
    let models = models_for_agent(agent);
    if models.is_empty() { return None; }
    let tier = match complexity {
        Complexity::Low => "cheap", Complexity::Medium => "standard", Complexity::High => "premium",
    };
    models.iter().find(|m| m.tier == tier).or_else(|| models.first()).map(|m| m.model)
}

const CODING_FALLBACK_CHAIN: &[AgentKind] = &[
    AgentKind::Codex, AgentKind::Cursor, AgentKind::OpenCode, AgentKind::Kilo,
];
pub(crate) fn coding_fallback_for(agent: &AgentKind) -> Option<AgentKind> {
    let available = detect_agents();
    let start = CODING_FALLBACK_CHAIN.iter().position(|k| k == agent)?;
    CODING_FALLBACK_CHAIN[start + 1..]
        .iter()
        .find(|k| available.contains(k) && !rate_limit::is_rate_limited(k))
        .copied()
}

fn team_override_score(team: &TeamConfig, agent_name: &str, category: TaskCategory) -> Option<i32> {
    let overrides = team.overrides.get(agent_name)?;
    match category {
        TaskCategory::Research => overrides.research,
        TaskCategory::SimpleEdit => overrides.simple_edit,
        TaskCategory::ComplexImpl => overrides.complex_impl,
        TaskCategory::Frontend => overrides.frontend,
        TaskCategory::Debugging => overrides.debugging,
        TaskCategory::Testing => overrides.testing,
        TaskCategory::Refactoring => overrides.refactoring,
        TaskCategory::Documentation => overrides.documentation,
    }
}

#[cfg(test)]
mod tests {
    use super::select_agent_from;
    use crate::agent::RunOpts;
    use crate::paths::{self, AidHomeGuard};
    use crate::store::Store;
    use crate::types::AgentKind;
    use std::fs;
    use rusqlite::params;
    use tempfile::TempDir;

    fn opts(dir: Option<&str>, budget: bool) -> RunOpts {
        RunOpts {
            dir: dir.map(|s| s.to_string()), output: None, model: None,
            budget, read_only: false, context_files: vec![], session_id: None,
        }
    }
    fn isolated() -> (TempDir, AidHomeGuard) {
        let temp = TempDir::new().unwrap();
        let guard = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(paths::aid_dir()).ok();
        (temp, guard)
    }
    fn select(prompt: &str, dir: &[&str], available: &[AgentKind]) -> (String, String) {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        select_agent_from(prompt, &opts(dir.first().copied(), false), available, &store, None)
    }
    fn all() -> [AgentKind; 5] {
        [AgentKind::Gemini, AgentKind::OpenCode, AgentKind::Kilo,
         AgentKind::Cursor, AgentKind::Codex]
    }
    fn insert_task(store: &Store, id: &str, agent: AgentKind, status: &str, cost: Option<f64>) {
        let conn = store.db();
        conn.execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, agent.as_str(), "history", status, "2026-03-15T00:00:00Z", cost],
        )
        .unwrap();
    }
    fn insert_task_with_prompt(store: &Store, id: &str, agent: AgentKind, status: &str, prompt: &str) {
        let conn = store.db();
        conn.execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                agent.as_str(),
                prompt,
                status,
                "2026-03-15T00:00:00Z",
                Option::<f64>::None
            ],
        )
        .unwrap();
    }
    fn seed_history(store: &Store, agent: AgentKind, successes: usize, failures: usize) {
        for i in 0..successes {
            let id = format!("history-{}-s-{}", agent.as_str(), i);
            insert_task(store, &id, agent, "done", None);
        }
        for i in 0..failures {
            let id = format!("history-{}-f-{}", agent.as_str(), i);
            insert_task(store, &id, agent, "failed", None);
        }
    }

    #[test]
    fn research_tasks_go_to_gemini() {
        let (kind, reason) = select(
            "Explain the authentication flow and compare the docs?", &[], &all(),
        );
        assert_eq!(kind, AgentKind::Gemini.as_str());
        assert!(reason.contains("research"));
        assert!(reason.contains("gemini"));
    }
    #[test]
    fn simple_edits_go_to_opencode() {
        let (kind, reason) = select(
            "rename src/types.rs field name to task_name", &[], &all(),
        );
        assert_eq!(kind, AgentKind::OpenCode.as_str());
        assert!(reason.contains("simple-edit"));
    }
    #[test]
    fn frontend_tasks_go_to_cursor() {
        let (kind, reason) = select(
            "Create a responsive React component layout for the settings UI",
            &["web/app.tsx"], &all(),
        );
        assert_eq!(kind, AgentKind::Cursor.as_str());
        assert!(reason.contains("frontend"));
    }

    #[test]
    fn custom_agent_scores_are_considered() {
        let (_temp, _guard) = isolated();
        let agents_dir = paths::aid_dir().join("agents");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::write(
            agents_dir.join("researcher.toml"),
            r#"[agent]
id = "researcher"
display_name = "Researcher Agent"
command = "true"
[agent.capabilities]
research = 12
"#,
        )
        .unwrap();
        let store = Store::open_memory().unwrap();
        let (kind, _) = select_agent_from(
            "Explain the authentication flow and compare the docs?",
            &opts(None, false), &all(), &store, None,
        );
        assert_eq!(kind, "researcher");
    }

    #[test]
    fn custom_agent_strengths_boost_score() {
        let (_temp, _guard) = isolated();
        let agents_dir = paths::aid_dir().join("agents");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::write(
            agents_dir.join("strengthy.toml"),
            r#"[agent]
id = "strengthy"
display_name = "Strengthy Agent"
command = "true"
strengths = ["research"]

[agent.capabilities]
research = 6
"#,
        )
        .unwrap();
        let store = Store::open_memory().unwrap();
        let (kind, _) = select_agent_from(
            "Explain the authentication flow and compare the docs?",
            &opts(None, false), &all(), &store, None,
        );
        assert_eq!(kind, "strengthy");
    }

    #[test]
    fn custom_strengths_not_matching_category() {
        let (_temp, _guard) = isolated();
        let agents_dir = paths::aid_dir().join("agents");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::write(
            agents_dir.join("docy.toml"),
            r#"[agent]
id = "docy"
display_name = "Doc Agent"
command = "true"
strengths = ["documentation"]

[agent.capabilities]
research = 6
"#,
        )
        .unwrap();
        let store = Store::open_memory().unwrap();
        let (kind, _) = select_agent_from(
            "Explain the authentication flow and compare the docs?",
            &opts(None, false), &all(), &store, None,
        );
        assert_eq!(kind, AgentKind::Gemini.as_str());
    }
    #[test]
    fn complex_tasks_go_to_codex() {
        let prompt = format!(
            "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. {}",
            "Add validation coverage and refactor the task dispatch flow. ".repeat(12)
        );
        let (kind, reason) = select(&prompt, &["src"], &all());
        assert_eq!(kind, AgentKind::Codex.as_str());
        assert!(reason.contains("complex-impl"));
        assert!(reason.contains("codex"));
    }
    #[test]
    fn unavailable_primary_agent_falls_back_to_next_best() {
        let (kind, reason) = select(
            "rename src/types.rs field name to task_name",
            &[], &[AgentKind::Gemini, AgentKind::Codex],
        );
        assert_eq!(kind, AgentKind::Codex.as_str());
        assert!(reason.contains("simple-edit"));
        assert!(reason.contains("opencode unavailable"));
    }
    #[test]
    fn budget_mode_avoids_codex_for_complex_tasks() {
        let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        // Seed cost data: codex is expensive, cursor is cheap
        for i in 0..5 {
            insert_task(&store, &format!("cost-codex-{i}"), AgentKind::Codex, "done", Some(2.50));
            insert_task(&store, &format!("cost-cursor-{i}"), AgentKind::Cursor, "done", Some(0.10));
        }
        let (kind, reason) = select_agent_from(prompt, &opts(Some("src"), true), &all(), &store, None);
        assert_ne!(kind, AgentKind::Codex.as_str());
        assert!(reason.contains("budget"));
    }
    #[test]
    fn budget_mode_prefers_kilo_for_simple_edits() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        let (kind, reason) = select_agent_from(
            "rename src/types.rs field name",
            &opts(Some("src"), true), &[AgentKind::Kilo, AgentKind::Codex], &store, None,
        );
        assert_eq!(kind, AgentKind::Kilo.as_str());
        assert!(reason.contains("budget"));
    }
    #[test]
    fn history_penalty_for_low_success_rate() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        seed_history(&store, AgentKind::Codex, 5, 5);
        let (kind, _) = select_agent_from(
            "add type annotation to field",
            &opts(Some("src"), false), &[AgentKind::OpenCode, AgentKind::Codex], &store, None,
        );
        assert_eq!(kind, AgentKind::OpenCode.as_str());
    }
    #[test]
    fn history_penalty_overrides_default_codex_selection() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        seed_history(&store, AgentKind::Codex, 5, 5);
        let (kind, _) = select_agent_from(
            "some random task", &opts(None, false),
            &[AgentKind::Codex, AgentKind::Gemini], &store, None,
        );
        assert_eq!(kind, AgentKind::Gemini.as_str());
    }
    #[test]
    fn history_bonus_for_high_success_rate() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        seed_history(&store, AgentKind::Codex, 10, 0);
        let (kind, _) = select_agent_from(
            "implement auth flow",
            &opts(None, false), &all(), &store, None,
        );
        assert_eq!(kind, AgentKind::Codex.as_str());
    }
    #[test]
    fn history_prefers_high_success_rate_over_base_score() {
        let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        seed_history(&store, AgentKind::Codex, 6, 4);
        seed_history(&store, AgentKind::Cursor, 19, 1);
        let (kind, _) = select_agent_from(
            prompt, &opts(None, false), &[AgentKind::Cursor, AgentKind::Codex], &store, None,
        );
        assert_eq!(kind, AgentKind::Cursor.as_str());
    }
    #[test]
    fn history_ignored_for_low_task_count() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        seed_history(&store, AgentKind::Gemini, 1, 2);
        let (kind, _) = select_agent_from(
            "research: what is MVP?", &opts(None, false), &all(), &store, None,
        );
        assert_eq!(kind, AgentKind::Gemini.as_str());
    }
    #[test]
    fn history_appears_in_reason() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        seed_history(&store, AgentKind::Gemini, 8, 2);
        let (_, reason) = select_agent_from("research: what is MVP?", &opts(None, false), &all(), &store, None);
        assert!(reason.contains("80% success"));
    }
    #[test]
    fn similar_tasks_hint_appended() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        let prompt = "Add routing hints for agent selection";
        for i in 0..3 {
            insert_task_with_prompt(
                &store,
                &format!("hint-done-{i}"),
                AgentKind::Codex,
                "done",
                "Implement routing hints for agent selection",
            );
        }
        insert_task_with_prompt(
            &store,
            "hint-fail",
            AgentKind::Codex,
            "failed",
            "Implement routing hints for agent selection",
        );
        let (_, reason) = select_agent_from(prompt, &opts(None, false), &all(), &store, None);
        assert!(reason.contains("similar tasks"));
        assert!(reason.contains("codex 3/4 success"));
    }
    #[test]
    fn similar_tasks_hint_absent_without_history() {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        let (_, reason) = select_agent_from(
            "Add routing hints for agent selection",
            &opts(None, false),
            &all(),
            &store,
            None,
        );
        assert!(!reason.contains("similar tasks"));
    }
    #[test]
    fn cost_efficiency_calculates_ratio() {
        let value = super::cost_efficiency(9.0, 1.5);
        assert!((value - 3.6).abs() < 1e-6);
    }
}
