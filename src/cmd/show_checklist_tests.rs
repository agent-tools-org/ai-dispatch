// Tests for checklist extraction and `aid show` checklist rendering.

use crate::cmd::show_checklist::{extract_checklist_from_prompt, render_checklist_status};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

fn sample_block(items: &[&str]) -> String {
    let mut lines = vec![
        "<aid-checklist>".to_string(),
        "header".to_string(),
        String::new(),
    ];
    for (i, t) in items.iter().enumerate() {
        lines.push(format!("[ ] {}. {}", i + 1, t));
    }
    lines.push("</aid-checklist>".to_string());
    lines.join("\n")
}

#[test]
fn extract_checklist_from_prompt_parses_items() {
    let prompt = sample_block(&["First item", "Second item"]);
    let got = extract_checklist_from_prompt(&prompt);
    assert_eq!(
        got,
        vec!["First item".to_string(), "Second item".to_string()]
    );
}

#[test]
fn render_all_confirmed_shows_checkmarks() {
    let store = Store::open_memory().unwrap();
    let out = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        out.path(),
        "1. First item — CONFIRMED\n2. Second item — CONFIRMED\n",
    )
    .unwrap();
    let task = Task {
        id: TaskId("t-cl-ok".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: sample_block(&["First item", "Second item"]),
        resolved_prompt: None,
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
        log_path: None,
        output_path: Some(out.path().display().to_string()),
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
    let text = render_checklist_status(&store, &task).expect("expected checklist");
    assert!(text.contains("Checklist: 2/2 addressed"));
    assert!(text.contains("✓ 1. First item — CONFIRMED"));
    assert!(text.contains("✓ 2. Second item — CONFIRMED"));
}

#[test]
fn render_missing_item_shows_x() {
    let store = Store::open_memory().unwrap();
    let out = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(out.path(), "1. Only first — CONFIRMED\n").unwrap();
    let task = Task {
        id: TaskId("t-cl-miss".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: sample_block(&["First item", "Second item"]),
        resolved_prompt: None,
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
        log_path: None,
        output_path: Some(out.path().display().to_string()),
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
    let text = render_checklist_status(&store, &task).expect("expected checklist");
    assert!(text.contains("Checklist: 1/2 addressed"));
    assert!(text.contains("✗ 2. Second item — MISSING"));
}

#[test]
fn render_without_checklist_returns_none() {
    let store = Store::open_memory().unwrap();
    let task = Task {
        id: TaskId("t-cl-none".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "no checklist here".to_string(),
        resolved_prompt: None,
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
    assert!(render_checklist_status(&store, &task).is_none());
}
