// Worker finalization regressions for stop winners and buffered cost failures.
// Drives real piped and PTY runners with a stop at completion parsing.
// Deps: isolated Store, agent adapter, stop command and shared price fixtures.

use super::stop;
use crate::agent::{Agent, RunOpts};
use crate::store::Store;
use crate::types::*;
use std::process::Command;
use std::sync::{Arc, Mutex};

struct FinalizingAgent {
    store: Arc<Store>,
    id: TaskId,
    stop_at_completion: bool,
    status: TaskStatus,
    stopped: Mutex<Option<Task>>,
}

impl Agent for FinalizingAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Gemini
    }
    fn streaming(&self) -> bool {
        true
    }
    fn accepts_interactive_input(&self) -> bool {
        false
    }
    fn build_command(&self, _: &str, _: &RunOpts) -> anyhow::Result<Command> {
        Ok(Command::new("true"))
    }
    fn parse_event(&self, _: &TaskId, _: &str) -> Option<TaskEvent> {
        None
    }
    fn parse_completion(&self, _: &str) -> CompletionInfo {
        if self.stop_at_completion {
            stop(&self.store, self.id.as_str()).expect("stop at finalization");
            *self.stopped.lock().expect("snapshot lock") =
                self.store.get_task(self.id.as_str()).expect("stopped task");
        }
        CompletionInfo {
            status: self.status,
            tokens: Some(1_000_000),
            model: Some("component-test-model".into()),
            cost_usd: None,
            exit_code: None,
        }
    }
}

fn agent(store: &Arc<Store>, stop_at_completion: bool, status: TaskStatus) -> FinalizingAgent {
    let mut task = crate::cost::test_support::task(AgentKind::Gemini, 0, None);
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("running task");
    FinalizingAgent {
        store: Arc::clone(store),
        id: task.id,
        stop_at_completion,
        status,
        stopped: Mutex::new(None),
    }
}

fn assert_stop_won(agent: &FinalizingAgent) {
    let stopped = agent.stopped.lock().expect("snapshot");
    let stopped = stopped.as_ref().expect("stop occurred");
    let task = agent
        .store
        .get_task(agent.id.as_str())
        .expect("query")
        .expect("task");
    assert_eq!(task.status, TaskStatus::Stopped);
    assert_eq!(task.completed_at, stopped.completed_at);
    assert_eq!(task.duration_ms, stopped.duration_ms);
    assert!(task.completed_at.is_some());
    assert!(task.duration_ms.is_some());
    let events = agent.store.get_events(agent.id.as_str()).expect("events");
    assert_eq!(
        events.len(),
        1,
        "only the winning stop completion survives: {events:?}"
    );
    assert_eq!(events[0].event_kind, EventKind::Completion);
    assert_eq!(events[0].detail, "Task stopped by user");
}

async fn run_piped(agent: &FinalizingAgent, streaming: bool, ceiling: Option<f64>) {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", "printf 'delivered\\n'; exit 0"]);
    crate::cmd::run::run_agent_process_with_cost(
        agent,
        cmd,
        &agent.id,
        &agent.store,
        &crate::support::paths::log_path(agent.id.as_str()),
        None,
        Some("component-test-model"),
        streaming,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        ceiling,
        std::time::Instant::now(),
    )
    .await
    .expect("piped runner");
}

async fn assert_finalization(pty: bool, streaming: bool, status: TaskStatus) {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard) = crate::cost::test_support::seed_prices(None);
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let agent = agent(&store, true, status);
    if pty {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "printf 'delivered\\n'; exit 0"]);
        crate::runtime::pty::runner::run_agent_process_with_control(
            &agent,
            &cmd,
            &agent.id,
            &store,
            &crate::support::paths::log_path(agent.id.as_str()),
            None,
            None,
            streaming,
            crate::config::timeout_policy::TimeoutPolicy::default(),
            None,
            None,
            std::time::Instant::now(),
        )
        .expect("PTY runner");
    } else {
        run_piped(&agent, streaming, Some(0.01)).await;
    }
    assert_stop_won(&agent);
}

#[tokio::test]
async fn streaming_stop_at_success_finalization_preserves_terminal_truth() {
    assert_finalization(false, true, TaskStatus::Done).await;
}

#[tokio::test]
async fn streaming_stop_at_failure_finalization_preserves_terminal_truth() {
    assert_finalization(false, true, TaskStatus::Failed).await;
}

#[tokio::test]
async fn pty_streaming_stop_at_success_finalization_preserves_terminal_truth() {
    assert_finalization(true, true, TaskStatus::Done).await;
}

#[tokio::test]
async fn pty_streaming_stop_at_failure_finalization_preserves_terminal_truth() {
    assert_finalization(true, true, TaskStatus::Failed).await;
}

#[tokio::test]
async fn pty_buffered_stop_at_success_finalization_preserves_terminal_truth() {
    assert_finalization(true, false, TaskStatus::Done).await;
}

#[tokio::test]
async fn pty_buffered_stop_at_failure_finalization_preserves_terminal_truth() {
    assert_finalization(true, false, TaskStatus::Failed).await;
}

#[tokio::test]
async fn buffered_exit_zero_estimated_cost_ceiling_keeps_failure_reason() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard) = crate::cost::test_support::seed_prices(None);
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let agent = agent(&store, false, TaskStatus::Done);
    run_piped(&agent, false, Some(0.01)).await;
    let task = store
        .get_task(agent.id.as_str())
        .expect("query")
        .expect("task");
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, Some(0));
    assert!(task.cost_usd.is_some_and(|cost| cost > 0.01));
    assert!(
        store
            .get_events(agent.id.as_str())
            .expect("events")
            .iter()
            .any(|event| {
                event.event_kind == EventKind::Error
                    && event.detail.contains("exceeded ceiling $0.01")
            }),
        "estimated cost failure must retain the ceiling reason"
    );
}

#[tokio::test]
async fn buffered_stop_at_cost_finalization_keeps_only_stop_reason() {
    assert_finalization(false, false, TaskStatus::Done).await;
}
