// Real buffered process regression for worker failure after an accepted stop.
// The adapter stops the task at the diagnostic/completion boundary.
// Deps: agent adapter trait, process runner and isolated stop fixtures.

use super::{stop, tests::make_task};
use crate::{
    agent::{Agent, RunOpts},
    support::paths,
    store::Store,
    types::*,
};
use std::process::Command;
use std::sync::Arc;

struct StopAtFailure {
    store: Arc<Store>,
    id: TaskId,
}

impl Agent for StopAtFailure {
    fn kind(&self) -> AgentKind {
        AgentKind::Grok
    }
    fn streaming(&self) -> bool {
        false
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
    fn diagnostics_report_terminal_failure(&self, _: &str) -> bool {
        stop(&self.store, self.id.as_str()).expect("stop before worker failure write");
        true
    }
}

#[tokio::test]
async fn settle_worker_failure_after_stop_records_no_failure_event() {
    let _permit = crate::support::test_subprocess::acquire();
    let _home = crate::support::test_env::TestHome::new();
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-stop-worker-writer".into());
    store
        .insert_task(&make_task(id.as_str(), TaskStatus::Running))
        .expect("task");
    let agent = StopAtFailure {
        store: Arc::clone(&store),
        id: id.clone(),
    };
    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", "exit 1"]);
    crate::cmd::run::run_agent_process_with_cost(
        false, &agent,
        cmd,
        &id,
        &store,
        &paths::log_path(id.as_str()),
        None,
        None,
        false,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
        std::time::Instant::now(),
    )
    .await
    .expect("worker exit");
    let task = store.get_task(id.as_str()).expect("get").expect("task");
    assert_eq!(task.status, TaskStatus::Stopped);
    assert!(task.completed_at.is_some());
    assert!(task.duration_ms.is_some());
    let events = store.get_events(id.as_str()).expect("events");
    assert_eq!(
        events.len(),
        1,
        "worker must not append a competing failure event"
    );
    assert_eq!(events[0].detail, "Task stopped by user");
}
