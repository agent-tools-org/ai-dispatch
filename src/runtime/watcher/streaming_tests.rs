// Streaming watcher integration tests.
// Covers process completion fields that must be returned to task persistence
// and OSC-prefixed line handling through the stream path (droid under PTY).
// Deps: watcher::watch_streaming, Store, Tokio process, stub and droid agents.

use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::agent::{Agent, RunOpts};
use crate::store::Store;
use crate::support::paths;
use crate::types::{
    AgentKind, CompletionInfo, EventKind, Task, TaskEvent, TaskId, TaskStatus, };


use super::watch_streaming;

struct StubStreamingAgent;

impl Agent for StubStreamingAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
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

pub(crate) fn insert_running_task(store: &Store, task_id: &TaskId) {
    store
        .insert_task(&Task {
            agent: AgentKind::Custom,
            ..crate::types::Task::test_fixture(task_id.clone(), TaskStatus::Running)
        })
        .unwrap();
}

#[path = "stream_stderr_tests.rs"]
mod stream_stderr_tests;

#[path = "stream_output_tests.rs"]
mod stream_output_tests;

#[tokio::test]
async fn droid_osc_prefixed_completion_line_yields_completion_event() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-droid-osc".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    // Real droid >=0.159 PTY output shape: OSC window-title and OSC 9;4
    // progress escapes glued to the front of stream-json lines.
    let script = r#"printf '\033]0;\342\233\254 reply pong\007{"type":"text","text":"pong"}\n'; printf '\033]9;4;0;\007{"type":"turn_complete","input_tokens":10,"output_tokens":5}\n'"#;
    let mut child = tokio::process::Command::new("sh")
        .current_dir(temp.path())
        .arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_streaming(
        &crate::agent::droid::DroidAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        crate::config::idle_timeout::DEFAULT_IDLE_TIMEOUT,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.tokens, Some(15));
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Completion
            && event.detail == "tokens: 10 in + 5 out = 15"
    }));
    assert!(events
        .iter()
        .any(|event| event.event_kind == EventKind::Reasoning && event.detail == "pong"));
}

#[tokio::test]
async fn streaming_watch_logs_report_containing_milestone_and_emits_event() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-milestone-report".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let standalone = "[MILESTONE] preliminary work complete";
    let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"[MILESTONE] implementation complete\n## Report\nThe full report remains available."}}"#;
    let mut child = tokio::process::Command::new("printf")
        .current_dir(temp.path())
        .args(["%s\n%s\n", standalone, line])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_streaming(
        &StubStreamingAgent, &mut child, &task_id, &store, &log_path, None,
        crate::config::idle_timeout::DEFAULT_IDLE_TIMEOUT, None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(std::fs::read_to_string(log_path).unwrap(), format!("{line}\n"));
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Milestone && event.detail == "preliminary work complete"
    }));
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Milestone && event.detail == "implementation complete"
    }));
}
