// Tests for the TUI App state machine.
// Covers filtering, milestone loading, detail mode navigation, and key handling.

use super::*;
use chrono::{Duration, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};

pub(crate) fn make_task(id: &str, group_id: Option<&str>) -> Task {
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
        repo_path: None, project_id: crate::project::current_project_id(),
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        // Fixture leaves model unattributed so route display keeps "unknown".
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
fn loaded_task_route_exposes_provider_and_attribution() {
    use crate::types::AttributionSource;
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-route-1", None);
    task.requested_model = Some("gpt-5.6".to_string());
    task.observed_model = Some("gpt-5.6".to_string());
    task.attribution_source = Some(AttributionSource::Echoed);
    store.insert_task(&task).unwrap();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: None,
        },
    )
    .unwrap();
    let loaded = app.tasks.iter().find(|t| t.id.as_str() == "t-route-1").unwrap();
    assert_eq!(loaded.route().provider.as_str(), "openai-chatgpt-plan");
    assert_eq!(loaded.display_route(), "codex/openai-chatgpt-plan/gpt-5.6");
    assert_eq!(loaded.attribution_source, Some(AttributionSource::Echoed));
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
    let label = app.scope_label();
    assert!(label.contains("project:"), "{label}");
    assert!(label.contains("today+active"), "{label}");
    assert!(label.contains("group wg-a"), "{label}");
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
fn default_scope_includes_active_tasks_from_previous_days() {
    let store = Arc::new(Store::open_memory().unwrap());
    let yesterday = Local::now() - Duration::days(1);
    let mut active_yesterday = make_task("t-2500", None);
    active_yesterday.status = TaskStatus::Running;
    active_yesterday.created_at = yesterday;
    store.insert_task(&active_yesterday).unwrap();

    let mut done_yesterday = make_task("t-2501", None);
    done_yesterday.created_at = yesterday;
    store.insert_task(&done_yesterday).unwrap();

    store.insert_task(&make_task("t-2502", None)).unwrap();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: None,
        },
    )
    .unwrap();

    let task_ids: Vec<&str> = app.tasks.iter().map(|task| task.id.as_str()).collect();
    assert!(task_ids.contains(&"t-2500"));
    assert!(task_ids.contains(&"t-2502"));
    assert!(!task_ids.contains(&"t-2501"));
}

#[test]
fn multipane_tasks_includes_pending_and_waiting_tasks() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut pending = make_task("t-3000", None);
    pending.status = TaskStatus::Pending;
    store.insert_task(&pending).unwrap();
    let mut waiting = make_task("t-3001", None);
    waiting.status = TaskStatus::Waiting;
    store.insert_task(&waiting).unwrap();

    let app = App::new(
        store,
        super::super::RunOptions {
            task_id: None,
            group: None,
        },
    )
    .unwrap();

    let task_ids: Vec<&str> = app
        .multipane_tasks()
        .iter()
        .map(|task| task.id.as_str())
        .collect();
    assert!(task_ids.contains(&"t-3000"));
    assert!(task_ids.contains(&"t-3001"));
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
    let label = app.scope_label();
    assert!(label.contains("project:"), "{label}");
    assert!(label.contains("task t-1001"), "{label}");
    assert!(label.contains("group wg-b"), "{label}");
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
    let mut task = make_task("t-1003", None);
    // Multi-line prompt so j can advance scroll past 0 under clamp.
    task.prompt = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    store.insert_task(&task).unwrap();
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
