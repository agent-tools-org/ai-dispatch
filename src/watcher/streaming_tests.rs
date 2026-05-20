// Streaming watcher integration tests.
// Covers process completion fields that must be returned to task persistence.
// Deps: watcher::watch_streaming, Store, Tokio process, and a stub Agent.

use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::agent::{Agent, RunOpts};
use crate::paths;
use crate::store::Store;
use crate::types::{
    AgentKind, CompletionInfo, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus,
};
use chrono::Local;

use super::watch_streaming;

struct StubStreamingAgent;
struct LoopingStreamingAgent;

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

impl Agent for LoopingStreamingAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }

    fn streaming(&self) -> bool {
        true
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

fn insert_running_task(store: &Store, task_id: &TaskId) {
    store
        .insert_task(&Task {
            id: task_id.clone(),
            agent: AgentKind::Custom,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Running,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
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
        })
        .unwrap();
}

#[tokio::test]
async fn streaming_watch_populates_success_exit_code() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-exit-code".to_string());
    insert_running_task(store.as_ref(), &task_id);
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

#[tokio::test]
async fn streaming_watch_loop_kill_reaches_exit_finalization() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-loop-kill".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("for i in 1 2 3 4 5 6 7 8 9 10 11 12; do printf 'repeat\\n'; done; sleep 5")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_streaming(
        &LoopingStreamingAgent,
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

    assert_eq!(info.status, TaskStatus::Failed);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.detail == super::loop_kill_detail(&task_id)));
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Error
            && event.detail.starts_with("FAIL")
            && event.detail.contains("exit code")
    }));
}
