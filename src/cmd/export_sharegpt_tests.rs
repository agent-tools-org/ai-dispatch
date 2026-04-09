// Tests for ShareGPT export serialization and transcript handling.
// Exports: module-local tests for export_sharegpt.rs.
// Deps: export_sharegpt helpers, Store, AidHomeGuard, tempfile.
use super::{ShareGptRecord, export_sharegpt};
use crate::paths::{self, AidHomeGuard};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use tempfile::TempDir;

#[test]
fn export_sharegpt_writes_system_human_gpt_messages() {
    let temp = TempDir::new().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let task = done_task("t-sharegpt", Some("resolved system prompt"));
    store.insert_task(&task).unwrap();
    write_transcript(task.id.as_str(), "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"agent reply\"}\n");

    let output = temp.path().join("sharegpt.jsonl");
    export_sharegpt(&store, task.id.as_str(), Some(output.to_str().unwrap())).unwrap();

    let record = read_record(&output);
    assert_eq!(record.conversations.len(), 3);
    assert_eq!(record.conversations[0].from, "system");
    assert_eq!(record.conversations[0].value, "resolved system prompt");
    assert_eq!(record.conversations[1].from, "human");
    assert_eq!(record.conversations[1].value, "user prompt");
    assert_eq!(record.conversations[2].from, "gpt");
    assert_eq!(record.conversations[2].value, "agent reply");
}

#[test]
fn export_sharegpt_formats_tool_calls_and_results() {
    let temp = TempDir::new().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let task = done_task("t-sharegpt-tools", Some("resolved system prompt"));
    store.insert_task(&task).unwrap();
    write_transcript(
        task.id.as_str(),
        concat!(
            "{\"type\":\"function_call\",\"name\":\"exec\",\"arguments\":{\"cmd\":\"cargo check\"}}\n",
            "{\"type\":\"tool_result\",\"tool_name\":\"exec\",\"output\":\"ok\"}\n"
        ),
    );

    let output = temp.path().join("sharegpt-tools.jsonl");
    export_sharegpt(&store, task.id.as_str(), Some(output.to_str().unwrap())).unwrap();

    let record = read_record(&output);
    assert!(record.conversations[2].value.contains("function_call: exec {\"cmd\":\"cargo check\"}"));
    assert!(record.conversations[2].value.contains("function_result: exec ok"));
}

#[test]
fn export_sharegpt_handles_empty_transcript() {
    let temp = TempDir::new().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let task = done_task("t-sharegpt-empty", Some("resolved system prompt"));
    store.insert_task(&task).unwrap();

    let output = temp.path().join("sharegpt-empty.jsonl");
    export_sharegpt(&store, task.id.as_str(), Some(output.to_str().unwrap())).unwrap();

    let record = read_record(&output);
    assert_eq!(record.conversations.len(), 3);
    assert_eq!(record.conversations[2].from, "gpt");
    assert!(record.conversations[2].value.is_empty());
}

#[test]
fn export_sharegpt_rejects_failed_tasks() {
    let temp = TempDir::new().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let mut task = done_task("t-sharegpt-failed", Some("resolved system prompt"));
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();

    let err = export_sharegpt(&store, task.id.as_str(), None).unwrap_err();

    assert!(err.to_string().contains("successful tasks"));
}

fn done_task(id: &str, resolved_prompt: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "user prompt".to_string(),
        resolved_prompt: resolved_prompt.map(str::to_string),
        category: None,
        status: TaskStatus::Done,
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
    }
}

fn write_transcript(task_id: &str, content: &str) {
    let path = paths::transcript_path(task_id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn read_record(path: &std::path::Path) -> ShareGptRecord {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
