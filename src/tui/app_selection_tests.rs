// Selection-identity and scroll-clamp regression tests for the TUI App.
// Isolates AID_HOME so tests never touch the developer's real ~/.aid.

use super::*;
use chrono::{Duration, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};

fn make_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: format!("prompt for {id}"),
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
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
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
    }
}

#[test]
fn selection_follows_task_identity_when_newer_task_appears() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());

    let mut older = make_task("t-sel-old");
    older.created_at = Local::now() - Duration::seconds(30);
    store.insert_task(&older).unwrap();
    let mut mid = make_task("t-sel-mid");
    mid.created_at = Local::now() - Duration::seconds(10);
    store.insert_task(&mid).unwrap();

    let mut app = App::new(
        store.clone(),
        super::super::RunOptions {
            task_id: None,
            group: None,
        },
    )
    .unwrap();

    // Newest-first list: select the older task explicitly.
    let old_idx = app
        .tasks
        .iter()
        .position(|t| t.id.as_str() == "t-sel-old")
        .expect("older task present");
    app.selected = old_idx;
    assert_eq!(
        app.selected_task().map(|t| t.id.as_str()),
        Some("t-sel-old")
    );

    // A newer task takes index 0; selection must stay on the same identity.
    let mut newest = make_task("t-sel-new");
    newest.created_at = Local::now();
    store.insert_task(&newest).unwrap();
    app.reload_tasks().unwrap();

    assert_eq!(
        app.selected_task().map(|t| t.id.as_str()),
        Some("t-sel-old"),
        "selection must follow task id across refresh, not positional index"
    );
    assert!(
        app.selected > 0,
        "newer task at index 0 should have shifted the older task down"
    );
}

#[test]
fn detail_scroll_clamps_to_content() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-scroll-1")).unwrap();
    let mut app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: None,
        },
    )
    .unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    app.detail_scroll = 10_000;
    app.clamp_detail_scroll();
    // Empty events list → max scroll is 0 (single placeholder line).
    assert_eq!(app.detail_scroll, 0);

    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
        .unwrap();
    app.detail_scroll = 10_000;
    app.clamp_detail_scroll();
    assert!(app.detail_scroll < 10_000);
}
