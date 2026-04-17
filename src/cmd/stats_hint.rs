// Diversification hint builder for `aid stats`.
// Exports: diversification_hint(). Deps: none.

pub fn diversification_hint(agent: &str, share_pct: usize, total_tasks: usize, filtered_agent: bool) -> Option<String> {
    const DOMINANT_AGENT_SHARE_THRESHOLD: usize = 70;
    const MIN_TASKS_FOR_HINT: usize = 5;

    if filtered_agent || total_tasks < MIN_TASKS_FOR_HINT || share_pct <= DOMINANT_AGENT_SHARE_THRESHOLD {
        return None;
    }
    Some(match agent {
        "codex" => format!("Tip: {agent} handled {share_pct}% of tasks. For simple edits, try `opencode` (5-20x cheaper). For research/docs, try `gemini -p \"...\"`."),
        "cursor" => format!("Tip: {agent} handled {share_pct}% of tasks. For backend-heavy work, try `codex`. For simple edits, try `opencode`."),
        _ => format!("Tip: {agent} handled {share_pct}% of tasks. Try `aid run auto <prompt>` to let aid pick the best agent per task."),
    })
}

#[cfg(test)]
mod tests {
    use super::diversification_hint;

    #[test]
    fn hint_fires_when_codex_crosses_concentration_threshold() {
        let hint = diversification_hint("codex", 80, 5, false);
        assert!(hint.as_ref().is_some_and(|value| value.contains("codex handled 80% of tasks")));
    }

    #[test]
    fn hint_is_suppressed_when_agent_filter_is_active() {
        assert_eq!(diversification_hint("codex", 80, 5, true), None);
    }

    #[test]
    fn hint_is_suppressed_when_not_enough_tasks_exist() {
        assert_eq!(diversification_hint("codex", 100, 4, false), None);
    }
}
