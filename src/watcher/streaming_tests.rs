// Streaming watcher integration tests.
// Covers process completion fields that must be returned to task persistence.
// Deps: watcher::watch_streaming, Store, Tokio process, and a stub Agent.

use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::agent::{Agent, RunOpts};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, TaskEvent, TaskId, TaskStatus};

use super::watch_streaming;

struct StubStreamingAgent;

impl Agent for StubStreamingAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, _prompt: &str, _opts: &RunOpts) -> anyhow::Result<Command> {
        Ok(Command::new("true"))
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    }
}

#[tokio::test]
async fn streaming_watch_populates_success_exit_code() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-exit-code".to_string());
    let log_path = temp.path().join("stream.log");
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("printf 'done\\n'; exit 0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_streaming(
        &StubStreamingAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.exit_code, Some(0));
}
