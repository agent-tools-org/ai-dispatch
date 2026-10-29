// Tests for ShareGPT export serialization and transcript handling.
// Exports: module-local tests for export_sharegpt.rs.
// Deps: export_sharegpt helpers, Store, AidHomeGuard, tempfile.
use super::{ShareGptRecord, export_sharegpt};
use crate::support::paths::{self};
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};
use chrono::Local;


#[test]
fn prompt_preservation_matches_show_and_sharegpt_stored_context() {
    let temp = crate::support::test_env::TestHome::new();
    let store = std::sync::Arc::new(Store::open_memory().unwrap());
    let group = store
        .create_workgroup("context", &"shared line\n".repeat(2000), None, None)
        .unwrap();
    let mut task = done_task("t-long-prompt", None);
    task.prompt = format!(
        "[Previous attempt feedback]\nfix everything\n\n[Original task]\n{}",
        "original task line\n".repeat(1000)
    );
    task.workgroup_id = Some(group.id.to_string());
    task.resolved_prompt = Some(crate::task::workgroup::compose_prompt(
        &task.prompt, None, Some(&group), &[], &[],
    ).text);
    store.insert_task(&task).unwrap();
    let output = temp.path().join("long-prompt.jsonl");
    export_sharegpt(&store, task.id.as_str(), Some(output.to_str().unwrap())).unwrap();
    let record = read_record(&output);
    let shown = crate::cmd::show::context_text(&store, task.id.as_str()).unwrap();
    assert!(shown.contains(&record.conversations[0].value));
    assert!(record.conversations[0].value.contains(&task.prompt));
    assert!(record.conversations[0].value.contains("Shared Context: "));
    assert!(record.conversations[0].value.contains("characters omitted"));
    assert_eq!(record.conversations[1].value, task.prompt);
    assert!(store.get_events(task.id.as_str()).unwrap().is_empty());
}

#[test]
fn export_sharegpt_writes_system_human_gpt_messages() {
    let temp = crate::support::test_env::TestHome::new();
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
    let temp = crate::support::test_env::TestHome::new();
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
    let temp = crate::support::test_env::TestHome::new();
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
fn export_sharegpt_fallback_includes_file_writes() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let task = done_task("t-sharegpt-file-write", Some("resolved system prompt"));
    store.insert_task(&task).unwrap();
    store
        .insert_event(&TaskEvent::new(
            task.id.clone(),
            EventKind::FileWrite,
            "Edit files: src/main.rs".to_string(),
        ))
        .unwrap();

    let output = temp.path().join("sharegpt-file-write.jsonl");
    export_sharegpt(&store, task.id.as_str(), Some(output.to_str().unwrap())).unwrap();

    let record = read_record(&output);
    assert_eq!(
        record.conversations[2].value,
        "function_call: Edit files: src/main.rs"
    );
}

#[test]
fn export_sharegpt_fallback_includes_file_reads() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let task = done_task("t-sharegpt-file-read", Some("resolved system prompt"));
    store.insert_task(&task).unwrap();
    store
        .insert_event(&TaskEvent::new(
            task.id.clone(),
            EventKind::FileRead,
            "Read file: src/main.rs".to_string(),
        ))
        .unwrap();

    let output = temp.path().join("sharegpt-file-read.jsonl");
    export_sharegpt(&store, task.id.as_str(), Some(output.to_str().unwrap())).unwrap();

    let record = read_record(&output);
    assert_eq!(
        record.conversations[2].value,
        "function_call: Read file: src/main.rs"
    );
}

#[test]
fn export_sharegpt_rejects_failed_tasks() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = done_task("t-sharegpt-failed", Some("resolved system prompt"));
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();

    let err = export_sharegpt(&store, task.id.as_str(), None).unwrap_err();

    assert!(err.to_string().contains("successful tasks"));
}

#[test]
fn export_sharegpt_rejects_hollow_delivery_done_tasks() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = done_task("t-sharegpt-hollow", Some("resolved system prompt"));
    task.delivery_assessment = Some(crate::types::DeliveryAssessment::HollowOutput);
    store.insert_task(&task).unwrap();

    let err = export_sharegpt(&store, task.id.as_str(), None).unwrap_err();

    assert!(err.to_string().contains("successful tasks"));
}

fn done_task(id: &str, resolved_prompt: Option<&str>) -> Task {
    Task {
        prompt: "user prompt".to_string(),
        resolved_prompt: resolved_prompt.map(str::to_string),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
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

#[test]
fn round2_preservation_show_export_use_stored_context_or_raw_legacy_prompt() {
    let temp = crate::support::test_env::TestHome::new();
    let store = std::sync::Arc::new(Store::open_memory().unwrap());
    let group = store.create_workgroup("context", "Context added after dispatch", None, None).unwrap();
    for (id, resolved) in [("t-stored", Some("Exact stored launch\né\n")), ("t-legacy", None)] {
        let mut task = done_task(id, resolved);
        task.prompt = "Caller brief with feedback\né\n".into();
        task.workgroup_id = Some(group.id.to_string());
        store.insert_task(&task).unwrap();
        let output = temp.path().join(format!("{id}.jsonl"));
        export_sharegpt(&store, id, Some(output.to_str().unwrap())).unwrap();
        let record = read_record(&output);
        let expected = resolved.unwrap_or(&task.prompt);
        assert_eq!(record.conversations[0].value, expected);
        assert_eq!(record.conversations[1].value, task.prompt);
        let shown = crate::cmd::show::context_text(&store, id).unwrap();
        assert!(shown.contains(expected));
        assert!(!shown.contains("Context added after dispatch"));
        assert!(!shown.contains("reconstructed"));
        assert!(store.get_events(id).unwrap().is_empty());
    }
}
