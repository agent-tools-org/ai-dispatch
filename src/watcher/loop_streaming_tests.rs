// Streaming watcher tests for automatic loop protection.
// Covers burst survival and sustained custom-text loop termination with an injected clock.

use std::cell::Cell;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Local;

use crate::agent::{Agent, RunOpts};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};

use super::streaming_tests::insert_running_task;
use super::{watch_streaming, watch_streaming_with_clock};

struct LoopingStreamingAgent {
    structured_events: bool,
}

impl Agent for LoopingStreamingAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }

    fn streaming(&self) -> bool {
        true
    }

    fn emits_structured_events(&self) -> bool {
        self.structured_events
    }

    fn build_command(&self, _prompt: &str, _opts: &RunOpts) -> anyhow::Result<Command> {
        Ok(Command::new("true"))
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: line.to_string(),
            metadata: None,
        })
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
async fn streaming_watch_fast_burst_reaches_exit_finalization_without_loop_kill() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-loop-burst".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = looping_child("for i in 1 2 3 4 5 6 7 8 9 10 11 12; do printf 'repeat\\n'; done");

    let info = watch_streaming(
        &LoopingStreamingAgent { structured_events: false },
        &mut child, &task_id, &store, &log_path, None,
        crate::idle_timeout::DEFAULT_IDLE_TIMEOUT, None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Done);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(!events.iter().any(|event| event.detail == super::loop_kill_detail(&task_id)));
}

#[tokio::test]
async fn streaming_watch_kills_sustained_custom_text_loop() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-loop-sustained".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = looping_child("while :; do printf 'repeat\\n'; done");
    let elapsed = Rc::new(Cell::new(0));
    let clock_elapsed = Rc::clone(&elapsed);
    let started_at = Instant::now();
    let clock = move || {
        let seconds = clock_elapsed.get();
        clock_elapsed.set(seconds + 1);
        started_at + Duration::from_secs(seconds)
    };

    let info = watch_streaming_with_clock(
        &LoopingStreamingAgent { structured_events: false },
        &mut child, &task_id, &store, &log_path, None,
        crate::idle_timeout::DEFAULT_IDLE_TIMEOUT, None, clock,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Failed);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.detail == super::loop_kill_detail(&task_id)));
}

#[tokio::test]
async fn streaming_watch_does_not_kill_structured_custom_narration() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-loop-structured-custom".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = looping_child(
        "i=0; while [ \"$i\" -lt 130 ]; do printf '{\"type\":\"text\",\"text\":\"repeat\"}\\n'; i=$((i + 1)); done",
    );
    let elapsed = Rc::new(Cell::new(0));
    let clock_elapsed = Rc::clone(&elapsed);
    let started_at = Instant::now();
    let clock = move || {
        let seconds = clock_elapsed.get();
        clock_elapsed.set(seconds + 1);
        started_at + Duration::from_secs(seconds)
    };

    let info = watch_streaming_with_clock(
        &LoopingStreamingAgent { structured_events: true },
        &mut child, &task_id, &store, &log_path, None,
        crate::idle_timeout::DEFAULT_IDLE_TIMEOUT, None, clock,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Done);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(!events.iter().any(|event| event.detail == super::loop_kill_detail(&task_id)));
}

fn looping_child(script: &str) -> tokio::process::Child {
    tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("looping test child should spawn")
}
