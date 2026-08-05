// Tests for foreground max-duration timeout policy.
// Covers active streams, idle deadline expiry, and default constant sharing.
// Deps: timeout helpers and shared config defaults.
use super::*;
use crate::agent::{Agent, RunOpts};
use crate::hooks::Hook;
use crate::types::{Task, VerifyStatus};
use std::process::Command as StdCommand;

struct TimeoutTestAgent;

impl Agent for TimeoutTestAgent {
    fn kind(&self) -> crate::types::AgentKind {
        crate::types::AgentKind::Codex
    }

    fn streaming(&self) -> bool {
        false
    }

    fn build_command(&self, _prompt: &str, _opts: &RunOpts) -> anyhow::Result<StdCommand> {
        Ok(StdCommand::new("sh"))
    }

    fn parse_event(
        &self,
        _task_id: &TaskId,
        _line: &str,
    ) -> Option<TaskEvent> {
        None
    }
}

#[test]
fn active_streaming_task_past_old_boundary_does_not_timeout() {
    let start = Instant::now();
    let old_boundary = Duration::from_millis(30);
    let now = start + old_boundary + Duration::from_millis(1);
    let last_activity = now - Duration::from_millis(5);

    assert!(!foreground_timeout_expired(
        start,
        last_activity,
        now,
        old_boundary,
        Duration::from_millis(10),
    ));
}

#[test]
fn idle_task_past_deadline_times_out() {
    let start = Instant::now();
    let max_duration = Duration::from_millis(30);
    let idle_timeout = Duration::from_millis(10);
    let now = start + max_duration + idle_timeout;

    assert!(foreground_timeout_expired(
        start,
        start,
        now,
        max_duration,
        idle_timeout,
    ));
}

#[test]
fn foreground_default_duration_uses_shared_config_constant() {
    assert_eq!(
        crate::timeout_policy::TimeoutPolicy::default().max_duration_mins(),
        crate::config::DEFAULT_MAX_TASK_DURATION_MINS
    );
}

#[tokio::test]
async fn max_duration_timeout_reaches_on_fail_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-foreground-timeout".to_string());
    store.insert_task(&task(&task_id, TaskStatus::Running)).unwrap();
    let log_path = temp.path().join("timeout.log");
    let hook_path = temp.path().join("hook.txt");
    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", "sleep 1"]);
    let policy = tiny_timeout_policy();

    run_agent_process_with_timeout(
        &TimeoutTestAgent,
        cmd,
        &task_id,
        &store,
        &log_path,
        None,
        None,
        false,
        None,
        policy,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    crate::cmd::run::post_run_lifecycle(
        crate::cmd::run::LifecycleMode::Foreground,
        &store,
        &task_id,
        &crate::cmd::run::RunArgs::default(),
        crate::types::AgentKind::Codex,
        "codex",
        None,
        None,
        None,
        None,
        &[Hook::new_trusted(
            "on_fail".to_string(),
            format!("printf failed > '{}'", hook_path.display()),
            None,
        )],
        &crate::cmd::run::PromptBundle {
            effective_prompt: "prompt".to_string(),
            context_files: Vec::new(),
            prompt_tokens: 0,
            injected_memory_ids: Vec::new(),
        },
        TaskStatus::Failed,
        None,
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(hook_path).unwrap(), "failed");
}

fn tiny_timeout_policy() -> crate::timeout_policy::TimeoutPolicy {
    crate::timeout_policy::TimeoutPolicy {
        idle: Duration::from_millis(20),
        first_token: Duration::from_millis(20),
        nudge_ladder: crate::timeout_policy::NudgeLadder {
            warn: Duration::from_millis(20),
            nudge: Duration::from_millis(20),
            escalate: Duration::from_millis(20),
        },
        max_duration: Duration::from_millis(20),
        hard_cap: Duration::from_secs(1),
    }
}

fn task(task_id: &TaskId, status: TaskStatus) -> Task {
    Task {
        id: task_id.clone(),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
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
        requested_model: None, observed_model: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
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
