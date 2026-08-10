// Hung recovery retry policy for idle-timeout failures.
// Exports retry limit, feedback text, and the auto-retry predicate.
// Deps: crate::process_monitor, crate::types.

use crate::process_monitor::HungContext;
use crate::types::{Task, TaskStatus};

pub const MAX_HUNG_RETRIES: u32 = 2;
pub const MAX_TRANSIENT_HUNG_RETRIES: u32 = 1;

pub fn build_hung_retry_feedback(task: &Task, hung_duration_secs: u64) -> String {
    let detail = task
        .resolved_prompt
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("last progress detail unavailable");
    let worktree_note = if task.worktree_path.is_some() {
        " Partial work is already committed in the worktree. Review what's there and continue."
    } else {
        ""
    };
    format!(
        "Previous attempt hung after {hung_duration_secs} seconds of no output. The task was working on: {detail}. Continue from where you stopped. If you're stuck on the same approach, try a different strategy.{worktree_note}"
    )
}

pub fn should_auto_retry_hung(task: &Task, context: &HungContext, retry_count: u32) -> bool {
    if task.status != TaskStatus::Failed {
        return false;
    }
    if context.transient {
        return retry_count < MAX_TRANSIENT_HUNG_RETRIES;
    }
    retry_count < MAX_HUNG_RETRIES
}

pub(crate) fn with_hung_context(task: &Task, context: &HungContext) -> Task {
    let mut enriched = task.clone();
    enriched.resolved_prompt = context.last_event_detail.clone();
    enriched.prompt_tokens = Some(i64::from(context.event_count));
    enriched
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::{AgentKind, TaskId, VerifyStatus};

    fn task() -> Task {
        Task {
            id: TaskId("t-hung".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Failed,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
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
            requested_model: None, observed_model: None, attribution_source: None,
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
    fn should_auto_retry_hung_for_progressing_task() {
        let task = task();
        let context = hung_context(6, false);
        assert!(should_auto_retry_hung(&task, &context, 0));
    }

    #[test]
    fn should_retry_hung_without_an_event_count_floor() {
        let task = task();
        let context = hung_context(5, false);
        assert!(should_auto_retry_hung(&task, &context, 0));
    }

    #[test]
    fn should_auto_retry_transient_without_progress_gate() {
        let task = task();
        let context = hung_context(1, true);
        assert!(should_auto_retry_hung(&task, &context, 0));
    }

    #[test]
    fn should_not_retry_transient_past_hung_limit() {
        let task = task();
        let context = hung_context(1, true);
        assert!(!should_auto_retry_hung(
            &task,
            &context,
            MAX_TRANSIENT_HUNG_RETRIES
        ));
    }

    #[test]
    fn build_hung_retry_feedback_includes_last_event_detail() {
        let mut task = task();
        task.resolved_prompt = Some("updating watcher timeout handling".to_string());
        let feedback = build_hung_retry_feedback(&task, 300);
        assert!(feedback.contains("updating watcher timeout handling"));
    }

    #[test]
    fn should_not_retry_past_hung_limit() {
        let task = task();
        let context = hung_context(9, false);
        assert!(!should_auto_retry_hung(&task, &context, MAX_HUNG_RETRIES));
    }

    fn hung_context(event_count: u32, transient: bool) -> HungContext {
        HungContext {
            hung_duration_secs: 300,
            event_count,
            last_event_detail: None,
            transient,
        }
    }
}
