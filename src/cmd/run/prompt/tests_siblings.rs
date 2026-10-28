// Extracted sibling context tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

#[test]
fn build_prompt_bundle_skips_sibling_context_for_opencode() {
    let _temp = crate::support::test_env::TestHome::with_dirs();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-opencode-test"))
        .unwrap();

    let sibling_task: crate::types::Task = crate::types::Task {
        agent: crate::types::AgentKind::Codex,
        prompt: "Sibling task prompt".to_string(),
        workgroup_id: Some(group.id.to_string()),
        tokens: Some(100),
        duration_ms: Some(1000),
        verify_status: crate::types::VerifyStatus::Skipped,
        ..crate::types::Task::test_fixture(crate::types::TaskId("task-sibling".to_string()), TaskStatus::Done)
    };
    store.insert_task(&sibling_task).unwrap();
    let summary = crate::cmd::summary::CompletionSummary {
        task_id: sibling_task.id.as_str().to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec!["src/lib.rs".to_string()],
        summary_text: "Task completed".to_string(),
        conclusion: String::new(),
        duration_secs: Some(1),
        token_count: Some(100),
    };
    store.save_completion_summary(sibling_task.id.as_str(), &serde_json::to_string(&summary).unwrap()).unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "opencode".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::OpenCode,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("Sibling Task Context"));
}

#[test]
fn build_prompt_bundle_skips_sibling_context_for_kilo() {
    let _temp = crate::support::test_env::TestHome::with_dirs();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-kilo-test"))
        .unwrap();

    let sibling_task: crate::types::Task = crate::types::Task {
        agent: crate::types::AgentKind::Codex,
        prompt: "Sibling task prompt".to_string(),
        workgroup_id: Some(group.id.to_string()),
        tokens: Some(100),
        duration_ms: Some(1000),
        verify_status: crate::types::VerifyStatus::Skipped,
        ..crate::types::Task::test_fixture(
            crate::types::TaskId("task-sibling-kilo".to_string()),
            TaskStatus::Done,
        )
    };
    store.insert_task(&sibling_task).unwrap();
    let summary = crate::cmd::summary::CompletionSummary {
        task_id: sibling_task.id.as_str().to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec!["src/lib.rs".to_string()],
        summary_text: "Task completed".to_string(),
        conclusion: String::new(),
        duration_secs: Some(1),
        token_count: Some(100),
    };
    store.save_completion_summary(sibling_task.id.as_str(), &serde_json::to_string(&summary).unwrap()).unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "kilo".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::Kilo,
        None,
        &[],
        "task-kilo",
        None,
        None,
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("Sibling Task Context"));
}

#[test]
fn build_prompt_bundle_includes_sibling_context_for_codex() {
    let _temp = crate::support::test_env::TestHome::with_dirs();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-codex-test"))
        .unwrap();

    let sibling_task: crate::types::Task = crate::types::Task {
        agent: crate::types::AgentKind::Codex,
        prompt: "Sibling task prompt".to_string(),
        workgroup_id: Some(group.id.to_string()),
        tokens: Some(100),
        duration_ms: Some(1000),
        verify_status: crate::types::VerifyStatus::Skipped,
        ..crate::types::Task::test_fixture(
            crate::types::TaskId("task-sibling-codex".to_string()),
            TaskStatus::Done,
        )
    };
    store.insert_task(&sibling_task).unwrap();
    let summary = crate::cmd::summary::CompletionSummary {
        task_id: sibling_task.id.as_str().to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec!["src/lib.rs".to_string()],
        summary_text: "Task completed".to_string(),
        conclusion: String::new(),
        duration_secs: Some(1),
        token_count: Some(100),
    };
    store.save_completion_summary(sibling_task.id.as_str(), &serde_json::to_string(&summary).unwrap()).unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::Codex,
        None,
        &[],
        "task-codex",
        None,
        None,
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("Sibling Task Context"));
}
