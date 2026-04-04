// Transcript persistence tests for buffered watcher execution.
// Covers raw buffer writes to task transcript files.
// Deps: watcher::watch_buffered, Agent trait, Store, tempfile.
use super::watch_buffered;
use crate::agent::{Agent, RunOpts};
use crate::paths;
use crate::store::Store;
use crate::types::{
    AgentKind, CompletionInfo, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus,
};
use chrono::Local;
use std::process::{Command, Stdio};
use std::sync::Arc;

struct BufferedTestAgent;

impl Agent for BufferedTestAgent {
    fn kind(&self) -> AgentKind { AgentKind::Gemini }
    fn streaming(&self) -> bool { false }
    fn build_command(&self, _: &str, _: &RunOpts) -> anyhow::Result<Command> { unreachable!() }
    fn parse_event(&self, _: &TaskId, _: &str) -> Option<TaskEvent> { None }
    fn parse_completion(&self, _: &str) -> CompletionInfo {
        CompletionInfo { tokens: None, status: TaskStatus::Done, model: None, cost_usd: None, exit_code: None }
    }
}

#[tokio::test]
async fn watch_buffered_persists_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task = Task {
        id: TaskId("t-watch-buffered".to_string()),
        agent: AgentKind::Gemini,
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
    };
    store.insert_task(&task).unwrap();
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("printf '{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"buffered transcript\"}\\n'")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

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
