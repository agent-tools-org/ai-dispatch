// Tests for foreground max-duration timeout policy.
// Covers active streams, idle deadline expiry, and default constant sharing.
// Deps: timeout helpers and shared config defaults.
use super::*;
use crate::agent::{Agent, RunOpts};
use crate::cmd::run::run_agent_process_with_cost as run_agent_process_with_timeout;
use crate::cmd::run::run_prompt;
use crate::hooks::Hook;
use crate::types::{Task, TaskEvent};
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};

#[path = "stop_launch_tests.rs"]
mod stop_launch_tests;

struct TimeoutTestAgent;

impl Agent for TimeoutTestAgent {
    fn kind(&self) -> crate::types::AgentKind {
        crate::types::AgentKind::Codex
    }

    fn streaming(&self) -> bool {
        false
    }

    fn accepts_interactive_input(&self) -> bool {
        false
    }

    fn build_command(&self, _prompt: &str, _opts: &RunOpts) -> anyhow::Result<StdCommand> {
        Ok(StdCommand::new("sh"))
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }
}

#[tokio::test]
async fn active_descendant_does_not_exempt_task_from_hard_timeout() {
    assert_worker_timeout("yes >/dev/null & wait").await;
}

#[tokio::test]
async fn active_streaming_task_times_out_at_hard_boundary() {
    assert_worker_timeout("while :; do echo progress; sleep .01; done").await;
}

#[tokio::test]
async fn idle_task_past_deadline_times_out() {
    assert_worker_timeout("sleep 10").await;
}

async fn assert_worker_timeout(script: &str) {
    let _permit = crate::support::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = TaskId("t-active-deadline".into());
    store.insert_task(&task(&task_id, TaskStatus::Running)).expect("task");
    let mut cmd = Command::new("sh");
    cmd.args(["-c", script]);
    run_agent_process_with_timeout(
        &TimeoutTestAgent,
        cmd,
        &task_id,
        &store,
        &temp.path().join("agent.log"),
        None,
        None,
        false,
        None,
        tiny_timeout_policy(),
        None,
    )
    .await
    .expect("worker");
    assert_eq!(store.get_task(task_id.as_str()).expect("query").expect("task").status, TaskStatus::Failed);
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
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::support::paths::ensure_dirs().unwrap();
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

    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    run_failure_hook(&store, &task_id, &hook_path).await;

    assert_eq!(std::fs::read_to_string(hook_path).unwrap(), "failed");
}

async fn run_failure_hook(store: &Arc<Store>, task_id: &TaskId, hook_path: &std::path::Path) {
    crate::cmd::run::post_run_lifecycle(
        crate::cmd::run::LifecycleMode::Foreground,
        store,
        task_id,
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
}

#[tokio::test]
async fn spawn_failure_marks_task_failed_not_running() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::support::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-spawn-miss".to_string());
    store.insert_task(&task(&task_id, TaskStatus::Running)).unwrap();
    let log_path = temp.path().join("spawn.log");
    let cmd = tokio::process::Command::new("/definitely/missing/agent-binary");
    let policy = crate::timeout_policy::TimeoutPolicy::default();

    let err = run_agent_process_with_timeout(
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
    .unwrap_err();

    assert!(err.to_string().contains("Failed to spawn agent process"), "unexpected error: {err}");
    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Failed);
    assert!(loaded.completed_at.is_some());
    let detail = store.latest_error(task_id.as_str()).unwrap();
    assert!(detail.contains("Failed during agent spawn"), "unexpected detail: {detail}");
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
    let mut task = crate::cost::test_support::task(crate::types::AgentKind::Codex, 0, None);
    task.id = task_id.clone();
    task.status = status;
    task
}

#[test]
fn process_foreground_completion_records_component_usage() {
    let _guard = crate::cost::test_support::seed_prices(Some(0.125));
    crate::support::paths::ensure_dirs().expect("aid dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = crate::cost::test_support::task(crate::types::AgentKind::Codex, 238_440, None);
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("task");
    let agent = crate::agent::codex::CodexAgent;
    let event = agent.parse_event(&task.id, crate::cost::test_support::CODEX_USAGE).expect("capture");
    store.insert_event(&event).expect("event");
    let info = CompletionInfo {
        tokens: task.tokens,
        status: TaskStatus::Done,
        model: task.requested_model.clone(),
        cost_usd: None,
        exit_code: Some(0),
    };
    let context = run_prompt::capture_failure_context(&store, &task.id, &Command::new("true"));
    let args = RunProcessArgs {
        agent: &agent,
        cmd: Command::new("true"),
        task_id: &task.id,
        store: &store,
        log_path: &crate::support::paths::log_path(task.id.as_str()),
        output_path: None,
        model: task.requested_model.as_deref(),
        streaming: true,
        workgroup_id: None,
        timeout_policy: crate::timeout_policy::TimeoutPolicy::default(),
        max_task_cost: None,
    };
    complete_agent(&args, &mut { info }, Instant::now(), &context).expect("record completion");
    let stored = store.get_task(task.id.as_str()).expect("query").expect("stored task");
    crate::cost::test_support::assert_cost(stored.cost_usd, 0.111981);
}
