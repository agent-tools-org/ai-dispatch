// Auto-selection heuristics for `aid run auto`.
// Scores prompt signals via capability matrix, respects installed CLIs, returns concise reason.
// Exports select_agent() helpers; deps: super::detect_agents, super::RunOpts.

use super::classifier::{self, Complexity, TaskCategory};
use super::{detect_agents, RunOpts};
use crate::rate_limit;
use crate::store::Store;
use crate::types::AgentKind;

const CAPABILITY: &[(AgentKind, &[(TaskCategory, i32)])] = &[
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
    (AgentKind::Ob1, &[
        (TaskCategory::Research, 5), (TaskCategory::ComplexImpl, 5),
        (TaskCategory::Debugging, 4), (TaskCategory::Testing, 4),
        (TaskCategory::SimpleEdit, 3), (TaskCategory::Frontend, 3),
        (TaskCategory::Refactoring, 4), (TaskCategory::Documentation, 3),
    ]),
    (AgentKind::Codebuff, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Frontend, 7), (TaskCategory::Testing, 6),
        (TaskCategory::Debugging, 6), (TaskCategory::SimpleEdit, 5),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
];

fn base_score(agent: AgentKind, category: TaskCategory) -> i32 {
    CAPABILITY.iter()
        .find(|(k, _)| *k == agent)
        .and_then(|(_, scores)| scores.iter().find(|(c, _)| *c == category))
        .map(|(_, s)| *s).unwrap_or(1)
}
fn is_cheap(agent: &AgentKind) -> bool {
    matches!(agent, AgentKind::Kilo | AgentKind::OpenCode | AgentKind::Gemini)
}
fn priority(kind: AgentKind) -> i32 {
    match kind {
        AgentKind::Gemini | AgentKind::Kilo | AgentKind::Ob1 => 0,
        AgentKind::OpenCode => 1, AgentKind::Cursor | AgentKind::Codebuff => 2, AgentKind::Codex => 3,
    }
}

pub(crate) fn select_agent_with_reason(
    prompt: &str, opts: &RunOpts, store: &Store,
) -> (AgentKind, String) {
    let history = store.agent_success_rates().unwrap_or_default();
    select_agent_from(prompt, opts, &detect_agents(), &history)
}

fn select_agent_from(
    prompt: &str, opts: &RunOpts, available: &[AgentKind],
    history: &[(AgentKind, f64, usize)],
) -> (AgentKind, String) {
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
        AgentKind::Cursor, AgentKind::Codex, AgentKind::Ob1,
    ];
    let score_for = |kind: AgentKind| -> i32 {
        let mut s = base_score(kind, profile.category);
        if budget {
            if is_cheap(&kind) { s += 4; } else { s -= 6; }
        }
        if rate_limit::is_rate_limited(&kind) { s -= 10; }
        if let Some((rate, count)) = history_map.get(&kind) {
            if *count >= 5 {
                if *rate < 0.65 { s -= 3; } else if *rate >= 0.90 { s += 2; }
            }
        }
        if matches!(profile.complexity, Complexity::High)
            && matches!(kind, AgentKind::Codex | AgentKind::Cursor)
        { s += 2; }
        s
    };
    let pick_best = |agents: &[AgentKind]| -> (AgentKind, i32) {
        agents.iter().map(|&k| (k, score_for(k)))
            .max_by_key(|&(k, s)| (s, priority(k)))
            .unwrap_or((AgentKind::Codex, 1))
    };
    let (primary_kind, _) = pick_best(&all_agents);
    let (sel_kind, sel_score) = if available.is_empty() {
        pick_best(&all_agents)
    } else {
        pick_best(available)
    };
    let mut reason = format!(
        "{} task ({}) \u{2192} {} (score: {})",
        profile.category.label(), profile.complexity.label(),
        sel_kind.as_str(), sel_score,
    );
    if sel_kind != primary_kind {
        reason.push_str(&format!("; {} unavailable", primary_kind.as_str()));
    }
    if auto_budget {
        reason.push_str("; auto-budget: low-value task");
    } else if opts.budget {
        reason.push_str("; budget mode");
    }
    if rate_limit::is_rate_limited(&AgentKind::Codex) && sel_kind != AgentKind::Codex {
        reason.push_str("; codex rate-limited");
    }
    if let Some((rate, count)) = history_map.get(&sel_kind) {
        if *count >= 5 {
            let percent = (*rate * 100.0).round() as i32;
            reason.push_str(&format!("; history: {}% success", percent));
        }
    }
    (sel_kind, reason)
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
    AgentKind::Codex, AgentKind::Cursor, AgentKind::OpenCode, AgentKind::Kilo, AgentKind::Ob1,
];
pub(crate) fn coding_fallback_for(agent: &AgentKind) -> Option<AgentKind> {
    let available = detect_agents();
    let start = CODING_FALLBACK_CHAIN.iter().position(|k| k == agent)?;
    CODING_FALLBACK_CHAIN[start + 1..]
        .iter()
        .find(|k| available.contains(k) && !rate_limit::is_rate_limited(k))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::select_agent_from;
    use crate::agent::RunOpts;
    use crate::paths;
    use crate::types::AgentKind;

    fn opts(dir: Option<&str>, budget: bool) -> RunOpts {
        RunOpts {
            dir: dir.map(|s| s.to_string()), output: None, model: None,
            budget, read_only: false, context_files: vec![], session_id: None,
        }
    }
    fn select(prompt: &str, dir: &[&str], available: &[AgentKind]) -> (AgentKind, String) {
        select_agent_from(prompt, &opts(dir.first().copied(), false), available, &[])
    }
    fn all() -> [AgentKind; 6] {
        [AgentKind::Gemini, AgentKind::OpenCode, AgentKind::Kilo,
         AgentKind::Cursor, AgentKind::Codex, AgentKind::Ob1]
    }
    fn with_history(prompt: &str, h: &[(AgentKind, f64, usize)]) -> (AgentKind, String) {
        select_agent_from(prompt, &opts(None, false), &all(), h)
    }

    #[test]
    fn research_tasks_go_to_gemini() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let (kind, reason) = select(
            "Explain the authentication flow and compare the docs?", &[], &all(),
        );
        assert_eq!(kind, AgentKind::Gemini);
        assert!(reason.contains("research"));
        assert!(reason.contains("gemini"));
    }
    #[test]
    fn simple_edits_go_to_opencode() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let (kind, reason) = select(
            "rename src/types.rs field name to task_name", &[], &all(),
        );
        assert_eq!(kind, AgentKind::OpenCode);
        assert!(reason.contains("simple-edit"));
    }
    #[test]
    fn frontend_tasks_go_to_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let (kind, reason) = select(
            "Create a responsive React component layout for the settings UI",
            &["web/app.tsx"], &all(),
        );
        assert_eq!(kind, AgentKind::Cursor);
        assert!(reason.contains("frontend"));
    }
    #[test]
    fn complex_tasks_go_to_codex() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let prompt = format!(
            "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. {}",
            "Add validation coverage and refactor the task dispatch flow. ".repeat(12)
        );
        let (kind, reason) = select(&prompt, &["src"], &all());
        assert_eq!(kind, AgentKind::Codex);
        assert!(reason.contains("complex-impl"));
        assert!(reason.contains("codex"));
    }
    #[test]
    fn unavailable_primary_agent_falls_back_to_next_best() {
        let (kind, reason) = select(
            "rename src/types.rs field name to task_name",
            &[], &[AgentKind::Gemini, AgentKind::Codex],
        );
        assert_eq!(kind, AgentKind::Codex);
        assert!(reason.contains("simple-edit"));
        assert!(reason.contains("opencode unavailable"));
    }
    #[test]
    fn budget_mode_avoids_codex_for_complex_tasks() {
        let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
        let (kind, reason) = select_agent_from(prompt, &opts(Some("src"), true), &all(), &[]);
        assert_ne!(kind, AgentKind::Codex);
        assert!(reason.contains("budget"));
    }
    #[test]
    fn budget_mode_prefers_kilo_for_simple_edits() {
        let (kind, reason) = select_agent_from(
            "rename src/types.rs field name",
            &opts(Some("src"), true), &[AgentKind::Kilo, AgentKind::Codex], &[],
        );
        assert_eq!(kind, AgentKind::Kilo);
        assert!(reason.contains("budget"));
    }
    #[test]
    fn history_penalty_for_low_success_rate() {
        let h = vec![(AgentKind::Codex, 0.50, 10)];
        let (kind, _) = select_agent_from(
            "add type annotation to field",
            &opts(Some("src"), false), &[AgentKind::OpenCode, AgentKind::Codex], &h,
        );
        assert_eq!(kind, AgentKind::OpenCode);
    }
    #[test]
    fn history_penalty_overrides_default_codex_selection() {
        let h = vec![(AgentKind::Codex, 0.50, 10)];
        let (kind, _) = select_agent_from(
            "some random task", &opts(None, false),
            &[AgentKind::Codex, AgentKind::Gemini], &h,
        );
        assert_eq!(kind, AgentKind::Gemini);
    }
    #[test]
    fn history_bonus_for_high_success_rate() {
        let (kind, _) = with_history("implement auth flow", &[(AgentKind::Codex, 0.95, 10)]);
        assert_eq!(kind, AgentKind::Codex);
    }
    #[test]
    fn history_ignored_for_low_task_count() {
        let (kind, _) = with_history("research: what is MVP?", &[(AgentKind::Gemini, 0.10, 3)]);
        assert_eq!(kind, AgentKind::Gemini);
    }
    #[test]
    fn history_appears_in_reason() {
        let h = vec![(AgentKind::Gemini, 0.80, 10)];
        let (_, reason) = select_agent_from(
            "research: what is MVP?", &opts(None, false), &all(), &h,
        );
        assert!(reason.contains("history:"));
        assert!(reason.contains("80% success"));
    }
}
