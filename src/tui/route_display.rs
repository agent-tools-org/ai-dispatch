// Width-aware formatting of a task's CLI / provider / model route for the TUI.
// Exports: format_route_fit. Deps: crate::types::Task (via display_route).

use crate::types::Task;

/// Format `cli/provider/model` for a given width budget.
///
/// Builds on `Task::display_route` so attribution grades stay consistent with
/// every other human surface. Truncation order (deliberate):
/// 1. Shrink the provider first — longest segment, least needed at a glance.
/// 2. Shrink the model next — attribution suffix may clip.
/// 3. Fall back to the CLI alone so the row never wraps into garbage.
///
/// Unknown model is always the literal `unknown`, never a guess.
pub fn format_route_fit(task: &Task, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let full = task.display_route();
    if char_len(&full) <= max_width {
        return full;
    }

    let cli = task.agent_display_name();
    // Split on the first two segments so attribution on the model is preserved
    // as one unit (e.g. "gpt-5.6 (inferred)" stays together).
    let mut parts = full.splitn(3, '/');
    let _cli_seg = parts.next().unwrap_or(cli);
    let provider = parts.next().unwrap_or("unknown");
    let model = parts.next().unwrap_or("unknown");

    // cli + two slashes already spent; remaining budget for provider+model.
    let fixed = char_len(cli) + 2;
    if fixed >= max_width {
        return truncate_chars(cli, max_width);
    }
    let remaining = max_width - fixed;

    // Prefer a readable provider stub over an empty one.
    let prov_budget = (remaining / 2).max(4).min(remaining.saturating_sub(1));
    let model_budget = remaining.saturating_sub(prov_budget);
    let prov = truncate_chars(provider, prov_budget);
    let modl = truncate_chars(model, model_budget.max(1));
    let fitted = format!("{cli}/{prov}/{modl}");
    if char_len(&fitted) <= max_width {
        return fitted;
    }
    truncate_chars(cli, max_width)
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if char_len(s) <= max {
        return s.to_string();
    }
    if max <= 3 {
        return s.chars().take(max).collect();
    }
    let keep = max - 3;
    let mut out: String = s.chars().take(keep).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, AttributionSource, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn task_with(
        agent: AgentKind,
        custom: Option<&str>,
        requested: Option<&str>,
        observed: Option<&str>,
        source: Option<AttributionSource>,
    ) -> Task {
        Task {
            id: TaskId("t-route".to_string()),
            agent,
            custom_agent_name: custom.map(str::to_string),
            prompt: "p".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None, project_id: crate::project::current_project_id(),
            worktree_path: None,
            worktree_branch: None,
            final_head_sha: None,
            final_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            requested_model: requested.map(str::to_string),
            observed_model: observed.map(str::to_string),
            attribution_source: source,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn full_route_shows_cli_provider_and_echoed_model() {
        let t = task_with(
            AgentKind::Codex,
            None,
            Some("gpt-5.6"),
            Some("gpt-5.6"),
            Some(AttributionSource::Echoed),
        );
        assert_eq!(
            format_route_fit(&t, 80),
            "codex/openai-chatgpt-plan/gpt-5.6"
        );
        assert_eq!(t.route().provider.as_str(), "openai-chatgpt-plan");
    }

    #[test]
    fn unknown_model_stays_unknown() {
        let t = task_with(AgentKind::Codex, None, None, None, None);
        assert_eq!(
            format_route_fit(&t, 80),
            "codex/openai-chatgpt-plan/unknown"
        );
        assert!(t.route().model.is_none());
    }

    #[test]
    fn inferred_attribution_is_visible() {
        let t = task_with(
            AgentKind::Codex,
            None,
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            Some(AttributionSource::ConfirmedBySuccess),
        );
        assert_eq!(
            format_route_fit(&t, 80),
            "codex/openai-chatgpt-plan/gpt-5.6-luna (inferred)"
        );
    }

    #[test]
    fn unconfirmed_request_is_marked() {
        let t = task_with(
            AgentKind::Cursor,
            None,
            Some("composer-2"),
            None,
            None,
        );
        let shown = format_route_fit(&t, 80);
        assert!(shown.ends_with("/composer-2?"), "{shown}");
        assert!(shown.starts_with("cursor/cursor-subscription/"), "{shown}");
    }

    #[test]
    fn custom_cli_name_is_used() {
        let t = task_with(AgentKind::Custom, Some("glm5"), None, None, None);
        assert_eq!(format_route_fit(&t, 80), "glm5/unknown/unknown");
    }

    #[test]
    fn narrow_width_keeps_cli_and_does_not_wrap() {
        let t = task_with(
            AgentKind::Qwen,
            None,
            Some("qwen3.8-max"),
            Some("qwen3.8-max"),
            Some(AttributionSource::Echoed),
        );
        let shown = format_route_fit(&t, 24);
        assert!(char_len(&shown) <= 24, "{shown}");
        assert!(shown.starts_with("qwen/"), "{shown}");
        assert!(!shown.contains('\n'));
    }

    #[test]
    fn substitution_shows_both_models() {
        let t = task_with(
            AgentKind::Cursor,
            None,
            Some("auto"),
            Some("composer-2"),
            Some(AttributionSource::Echoed),
        );
        let shown = format_route_fit(&t, 80);
        assert!(shown.contains("composer-2 (asked auto)"), "{shown}");
    }
}
