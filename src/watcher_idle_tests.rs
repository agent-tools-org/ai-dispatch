// Watchdog regressions driven by a fake agent with silent tool descendants.
// Covers CPU progress, sleeping children, and remote build-box lock waits.
// Deps: streaming watcher, subprocess groups, isolated Store and temporary scripts.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::agent::{Agent, RunOpts};
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, TaskEvent, TaskId, TaskStatus};

pub(crate) struct FakeAgent;

impl Agent for FakeAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }
    fn streaming(&self) -> bool {
        true
    }
    fn accepts_interactive_input(&self) -> bool {
        false
    }
    fn build_command(&self, _: &str, _: &RunOpts) -> anyhow::Result<Command> {
        Ok(Command::new("sh"))
    }
    fn parse_event(&self, _: &TaskId, _: &str) -> Option<TaskEvent> {
        None
    }
    fn parse_completion(&self, _: &str) -> CompletionInfo {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    }
}

async fn watch_silent_child(script: &str) -> (CompletionInfo, Vec<TaskEvent>) {
    let _permit = crate::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = TaskId("t-silent-child".into());
    super::streaming_tests::insert_running_task(&store, &task_id);
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", script])
        .current_dir(temp.path())
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fake agent");
    let info = super::watch_streaming(
        &FakeAgent,
        &mut child,
        &task_id,
        &store,
        &temp.path().join("agent.log"),
        None,
        Duration::from_millis(400),
        None,
    )
    .await
    .expect("watch");
    (info, store.get_events(task_id.as_str()).expect("events"))
}

#[tokio::test]
async fn silent_cpu_using_descendant_survives_idle_window() {
    let (info, events) = watch_silent_child(
        "echo started; yes >/dev/null & tool=$!; sleep 2; kill $tool; wait $tool; exit 0",
    )
    .await;
    assert_eq!(info.status, TaskStatus::Done, "{events:?}");
    assert_eq!(info.exit_code, Some(0));
}

#[tokio::test]
async fn silent_sleeping_descendant_is_reaped_after_idle_window() {
    let (info, events) = watch_silent_child("echo started; sleep 10 & wait").await;
    assert_eq!(info.status, TaskStatus::Failed);
    assert!(
        events
            .iter()
            .any(|event| event.detail.starts_with("Agent hung:"))
    );
}

#[tokio::test]
async fn silent_remote_lock_wait_survives_idle_window() {
    let (info, events) = watch_silent_child(
        "printf '#!/bin/sh\\nsleep 2\\n' > rbox; chmod +x rbox; echo started; ./rbox task --wait & wait",
    ).await;
    assert_eq!(info.status, TaskStatus::Done, "{events:?}");
}

#[tokio::test]
async fn silent_cpu_descendant_does_not_extend_first_output_timeout() {
    let (info, _) = watch_silent_child("yes >/dev/null & wait").await;
    assert_eq!(info.status, TaskStatus::Failed);
}
