// Transcript persistence tests for buffered watcher execution.
// Covers raw buffer writes to task transcript files.
// Deps: watcher::watch_buffered, Agent trait, Store, tempfile.
use super::watch_buffered;
use crate::agent::{Agent, RunOpts};
use crate::support::paths;
use crate::routing::rate_limit;
use crate::store::Store;
use crate::types::{
    AgentKind, CompletionInfo, Task, TaskEvent, TaskId, TaskStatus, };

use std::process::{Command, Stdio};
use std::sync::Arc;

struct BufferedTestAgent;

impl Agent for BufferedTestAgent {
    fn kind(&self) -> AgentKind { AgentKind::Antigravity }
    fn streaming(&self) -> bool { false }
    fn accepts_interactive_input(&self) -> bool { false }
    fn build_command(&self, _: &str, _: &RunOpts) -> anyhow::Result<Command> { unreachable!() }
    fn parse_event(&self, _: &TaskId, _: &str) -> Option<TaskEvent> { None }
}

#[tokio::test]
async fn watch_buffered_persists_transcript() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = Task {
        agent: AgentKind::Antigravity,
        ..crate::types::Task::test_fixture(TaskId("t-watch-buffered".to_string()), TaskStatus::Running)
    };
    store.insert_task(&task).unwrap();
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "printf '{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"buffered transcript\"}\\n'",
    );

    watch_buffered(
        &BufferedTestAgent,
        &mut child,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(paths::transcript_path(task.id.as_str())).unwrap(),
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"buffered transcript\"}\n"
    );
}

#[tokio::test]
async fn watch_buffered_clears_rate_limit_on_success() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = Task {
        agent: AgentKind::Antigravity,
        ..crate::types::Task::test_fixture(
            TaskId("t-watch-clear-rate-limit".to_string()),
            TaskStatus::Running,
        )
    };
    store.insert_task(&task).unwrap();
    rate_limit::mark_rate_limited(
        &AgentKind::Antigravity,
        None,
        "Individual quota reached. Resets in 59m21s.",
    );
    assert!(rate_limit::is_rate_limited(&AgentKind::Antigravity, None));

    let mut child = crate::support::test_command::piped_shell(temp.path(), "printf 'done\\n'");

    watch_buffered(
        &BufferedTestAgent,
        &mut child,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
    )
    .await
    .unwrap();

    assert!(!rate_limit::is_rate_limited(&AgentKind::Antigravity, None));
}

struct BufferedAgyTestAgent;

impl Agent for BufferedAgyTestAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Antigravity
    }
    fn streaming(&self) -> bool {
        false
    }
    fn accepts_interactive_input(&self) -> bool {
        false
    }
    fn build_command(&self, _: &str, _: &RunOpts) -> anyhow::Result<Command> {
        unreachable!()
    }
    fn parse_event(&self, _: &TaskId, _: &str) -> Option<TaskEvent> {
        None
    }
}

#[tokio::test]
async fn watch_buffered_records_quota_refusal_and_fails_task() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = Task {
        agent: AgentKind::Antigravity,
        requested_model: Some("gemini-3.6-flash".to_string()),
        ..crate::types::Task::test_fixture(TaskId("t-watch-buffered-quota".to_string()), TaskStatus::Running)
    };
    store.insert_task(&task).unwrap();

    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        r#"printf '{"type":"error","message":"Individual quota reached."}\n'"#,
    );

    let info = watch_buffered(
        &BufferedAgyTestAgent,
        &mut child,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Failed);
    assert!(rate_limit::is_group_rate_limited(&AgentKind::Antigravity, None, "gemini"));
}
