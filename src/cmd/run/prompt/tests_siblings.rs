// Extracted sibling context tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

#[test]
fn build_prompt_bundle_skips_sibling_context_for_opencode() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-opencode-test"))
        .unwrap();

    let sibling_task: crate::types::Task = crate::types::Task {
        id: crate::types::TaskId("task-sibling".to_string()),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Sibling task prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.id.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(100),
        prompt_tokens: None,
        duration_ms: Some(1000),
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-kilo-test"))
        .unwrap();

    let sibling_task: crate::types::Task = crate::types::Task {
        id: crate::types::TaskId("task-sibling-kilo".to_string()),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Sibling task prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.id.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(100),
        prompt_tokens: None,
        duration_ms: Some(1000),
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-codex-test"))
        .unwrap();

    let sibling_task: crate::types::Task = crate::types::Task {
        id: crate::types::TaskId("task-sibling-codex".to_string()),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Sibling task prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.id.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(100),
        prompt_tokens: None,
        duration_ms: Some(1000),
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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
