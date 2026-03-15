// Auto-selection heuristics for `aid run auto`.
// Scores prompt signals via capability matrix, respects installed CLIs, returns concise reason.
// Exports select_agent() helpers; deps: super::detect_agents, super::RunOpts.

use super::classifier::{self, Complexity, TaskCategory};
use super::{detect_agents, RunOpts};
use crate::rate_limit;
use crate::store::Store;
use crate::types::AgentKind;
use crate::agent::custom::CustomAgentConfig;
use crate::agent::registry::load_custom_agents;
use crate::team::TeamConfig;
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
fn is_cheap(agent: &AgentKind) -> bool {
    matches!(agent, AgentKind::Kilo | AgentKind::OpenCode | AgentKind::Gemini)
}
fn priority(kind: AgentKind) -> i32 {
    match kind {
        AgentKind::Gemini | AgentKind::Kilo => 0,
        AgentKind::OpenCode => 1, AgentKind::Cursor | AgentKind::Codebuff => 2, AgentKind::Codex => 3,
        AgentKind::Custom => 1,
    }
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

pub(crate) fn select_agent_with_reason(
    prompt: &str, opts: &RunOpts, store: &Store,
    team: Option<&TeamConfig>,
) -> (String, String) {
    let history = store.agent_success_rates().unwrap_or_default();
    let available = detect_agents();
    select_agent_from(prompt, opts, &available, &history, team)
}

fn select_agent_from(
    prompt: &str, opts: &RunOpts, available: &[AgentKind],
    history: &[(AgentKind, f64, usize)],
    team: Option<&TeamConfig>,
) -> (String, String) {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = classifier::count_file_mentions(&normalized);
    let profile = classifier::classify(prompt, file_count, prompt_len);
    let auto_budget = classifier::contains_any(&normalized, classifier::LOW_VALUE_TERMS);
    let budget = opts.budget || auto_budget;
    let history_map: std::collections::HashMap<AgentKind, (f64, usize)> = history
        .iter().map(|(k, r, c)| (*k, (*r, *c))).collect();
    let all_agents = [
        AgentKind::Gemini, AgentKind::OpenCode, AgentKind::Kilo,
        AgentKind::Cursor, AgentKind::Codex,
    ];
    let score_for = |kind: AgentKind| -> i32 {
        let mut s = if let Some(tc) = &team {
            team_override_score(tc, kind.as_str(), profile.category)
                .unwrap_or_else(|| base_score(kind, profile.category))
        } else {
            base_score(kind, profile.category)
        };
        if budget {
            if is_cheap(&kind) { s += 4; } else { s -= 6; }
        }
        if rate_limit::is_rate_limited(&kind) { s -= 10; }
        if let Some((rate, count)) = history_map.get(&kind) {
            if *count >= 5 {
                let bonus = ((*rate - 0.75) * 16.0).round() as i32;
                let bonus = bonus.clamp(-5, 4);
                s += bonus;
            }
        }
        if matches!(profile.complexity, Complexity::High)
            && matches!(kind, AgentKind::Codex | AgentKind::Cursor)
        { s += 2; }
        // Boost preferred agents from team (soft preference, not hard filter)
        if let Some(tc) = &team {
            if tc.preferred_agents.iter().any(|a| a.eq_ignore_ascii_case(kind.as_str())) {
                s += 3;
            }
        }
        s
    };
    let team_default = team.and_then(|t| t.default_agent.as_deref())
        .and_then(AgentKind::parse_str);
    let pick_best = |agents: &[AgentKind]| -> (AgentKind, i32) {
        agents.iter().map(|&k| {
            let s = score_for(k);
            let is_default = team_default == Some(k);
            (k, s, is_default)
        })
            .max_by_key(|&(k, s, is_default)| (s, is_default as i32, priority(k)))
            .map(|(k, s, _)| (k, s))
            .unwrap_or((AgentKind::Codex, 1))
    };
    let (primary_kind, _) = pick_best(&all_agents);
    let (available_kind, available_score) = if available.is_empty() {
        pick_best(&all_agents)
    } else {
        pick_best(available)
    };
    let mut selected_name = available_kind.as_str().to_string();
    let mut selected_score = available_score;
    let mut selected_builtin = Some(available_kind);
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
        let available_priority = priority(available_kind);
        if (custom_score, custom_priority) > (available_score, available_priority) {
            selected_name = custom_name;
            selected_score = custom_score;
            selected_builtin = None;
        }
    }
    let mut reason = format!(
        "{} task ({}) \u{2192} {} (score: {})",
        profile.category.label(), profile.complexity.label(),
        selected_name, selected_score,
    );
    if let Some(sel_kind) = selected_builtin {
        if sel_kind != primary_kind {
            reason.push_str(&format!("; {} unavailable", primary_kind.as_str()));
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
    (selected_name, reason)
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
    use crate::types::AgentKind;
    use std::fs;
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
        select_agent_from(prompt, &opts(dir.first().copied(), false), available, &[], None)
    }
    fn all() -> [AgentKind; 5] {
        [AgentKind::Gemini, AgentKind::OpenCode, AgentKind::Kilo,
         AgentKind::Cursor, AgentKind::Codex]
    }
    fn with_history(prompt: &str, h: &[(AgentKind, f64, usize)]) -> (String, String) {
        let (_temp, _guard) = isolated();
        select_agent_from(prompt, &opts(None, false), &all(), h, None)
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
        let (kind, _) = select_agent_from(
            "Explain the authentication flow and compare the docs?",
            &opts(None, false), &all(), &[], None,
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
        let (kind, _) = select_agent_from(
            "Explain the authentication flow and compare the docs?",
            &opts(None, false), &all(), &[], None,
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
        let (kind, _) = select_agent_from(
            "Explain the authentication flow and compare the docs?",
            &opts(None, false), &all(), &[], None,
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
        let (kind, reason) = select_agent_from(prompt, &opts(Some("src"), true), &all(), &[], None);
        assert_ne!(kind, AgentKind::Codex.as_str());
        assert!(reason.contains("budget"));
    }
    #[test]
    fn budget_mode_prefers_kilo_for_simple_edits() {
        let (_temp, _guard) = isolated();
        let (kind, reason) = select_agent_from(
            "rename src/types.rs field name",
            &opts(Some("src"), true), &[AgentKind::Kilo, AgentKind::Codex], &[], None,
        );
        assert_eq!(kind, AgentKind::Kilo.as_str());
        assert!(reason.contains("budget"));
    }
    #[test]
    fn history_penalty_for_low_success_rate() {
        let (_temp, _guard) = isolated();
        let h = vec![(AgentKind::Codex, 0.50, 10)];
        let (kind, _) = select_agent_from(
            "add type annotation to field",
            &opts(Some("src"), false), &[AgentKind::OpenCode, AgentKind::Codex], &h, None,
        );
        assert_eq!(kind, AgentKind::OpenCode.as_str());
    }
    #[test]
    fn history_penalty_overrides_default_codex_selection() {
        let (_temp, _guard) = isolated();
        let h = vec![(AgentKind::Codex, 0.50, 10)];
        let (kind, _) = select_agent_from(
            "some random task", &opts(None, false),
            &[AgentKind::Codex, AgentKind::Gemini], &h, None,
        );
        assert_eq!(kind, AgentKind::Gemini.as_str());
    }
    #[test]
    fn history_bonus_for_high_success_rate() {
        let (kind, _) = with_history("implement auth flow", &[(AgentKind::Codex, 0.95, 10)]);
        assert_eq!(kind, AgentKind::Codex.as_str());
    }
    #[test]
    fn history_prefers_high_success_rate_over_base_score() {
        let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
        let history = vec![
            (AgentKind::Codex, 0.60, 10),
            (AgentKind::Cursor, 0.95, 10),
        ];
        let (kind, _) = select_agent_from(
            prompt, &opts(None, false), &[AgentKind::Cursor, AgentKind::Codex], &history, None,
        );
        assert_eq!(kind, AgentKind::Cursor.as_str());
    }
    #[test]
    fn history_ignored_for_low_task_count() {
        let (kind, _) = with_history("research: what is MVP?", &[(AgentKind::Gemini, 0.10, 3)]);
        assert_eq!(kind, AgentKind::Gemini.as_str());
    }
    #[test]
    fn history_appears_in_reason() {
        let (_temp, _guard) = isolated();
        let h = vec![(AgentKind::Gemini, 0.80, 10)];
        let (_, reason) = select_agent_from(
            "research: what is MVP?", &opts(None, false), &all(), &h, None,
        );
        assert!(reason.contains("history:"));
        assert!(reason.contains("80% success"));
    }
}
