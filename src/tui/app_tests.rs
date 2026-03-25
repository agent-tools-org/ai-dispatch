// Tests for the TUI App state machine.
// Covers filtering, milestone loading, detail mode navigation, and key handling.

use super::*;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};

fn make_task(id: &str, group_id: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: format!("prompt for {id}"),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: group_id.map(str::to_string),
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
        }
}

#[test]
fn filters_today_view_by_group() {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&make_task("t-1000", Some("wg-a")))
        .unwrap();
    store
        .insert_task(&make_task("t-1001", Some("wg-b")))
        .unwrap();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: Some("wg-a".to_string()),
        },
    )
    .unwrap();

    assert_eq!(app.tasks.len(), 1);
    assert_eq!(app.tasks[0].id.as_str(), "t-1000");
    assert_eq!(app.scope_label(), "today | group wg-a");
}

#[test]
fn keeps_ungrouped_tasks_visible_with_group_filter() {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&make_task("t-2000", Some("wg-test")))
        .unwrap();
    store
        .insert_task(&make_task("t-2001", Some("wg-other")))
        .unwrap();
    store.insert_task(&make_task("t-2002", None)).unwrap();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: Some("wg-test".to_string()),
        },
    )
    .unwrap();

    let mut task_ids: Vec<&str> = app.tasks.iter().map(|task| task.id.as_str()).collect();
    task_ids.sort();
    assert_eq!(task_ids, vec!["t-2000", "t-2002"]);
}

#[test]
fn filters_specific_task_scope() {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&make_task("t-1000", Some("wg-a")))
        .unwrap();
    store
        .insert_task(&make_task("t-1001", Some("wg-b")))
        .unwrap();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: Some("t-1001".to_string()),
            group: Some("wg-b".to_string()),
        },
    )
    .unwrap();

    assert_eq!(app.tasks.len(), 1);
    assert_eq!(app.tasks[0].id.as_str(), "t-1001");
    assert_eq!(app.scope_label(), "task t-1001 | group wg-b");
}

#[test]
fn loads_running_task_milestone() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-1002", Some("wg-a"));
    task.status = TaskStatus::Running;
    store.insert_task(&task).unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: crate::types::EventKind::Milestone,
            detail: "types defined".to_string(),
            metadata: None,
        })
        .unwrap();

    let mut completed_task = make_task("t-1003", Some("wg-a"));
    completed_task.status = TaskStatus::Done;
    store.insert_task(&completed_task).unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: completed_task.id.clone(),
            timestamp: Local::now(),
            event_kind: crate::types::EventKind::Milestone,
            detail: "finished milestone".to_string(),
            metadata: None,
        })
        .unwrap();
    let completed_task_id = completed_task.id.clone();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: Some("wg-a".to_string()),
        },
    )
    .unwrap();

    assert_eq!(app.get_milestone("t-1002"), Some("types defined"));
    assert_eq!(app.get_milestone(completed_task_id.as_str()), Some("finished milestone"));
}

#[test]
fn detail_mode_cycles_tabs_and_resets_scroll() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-1003", None)).unwrap();
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
    assert!(app.detail_mode);
    assert!(app.detail_tab == DetailTab::Events);

    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
        .unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.detail_tab == DetailTab::Prompt);
    assert_eq!(app.detail_scroll, 1);

    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .unwrap();
    assert!(app.detail_tab == DetailTab::Output);
    assert_eq!(app.detail_scroll, 0);

    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.detail_tab == DetailTab::Prompt);
}

#[test]
fn detail_mode_keeps_selection_stable_and_resets_on_escape() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-1004", None)).unwrap();
    store.insert_task(&make_task("t-1005", None)).unwrap();
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
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected, 0);
    assert_eq!(app.detail_scroll, 0);

    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE))
        .unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(app.selected, 0);
    assert_eq!(app.detail_scroll, 0);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.detail_mode);
    assert!(app.detail_tab == DetailTab::Events);
    assert_eq!(app.detail_scroll, 0);
}
