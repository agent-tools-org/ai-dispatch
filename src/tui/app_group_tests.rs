// Regression tests for all-project visibility, grouped navigation, and search.
// Covers the operator-facing task board state machine.
// Deps: TUI App, in-memory Store, and Task fixtures.

use super::*;
use chrono::{Duration, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};

fn task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: format!("prompt {id}"), resolved_prompt: None, category: None,
        status: TaskStatus::Done, parent_task_id: None,
        workgroup_id: None, caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, project_id: None, worktree_path: None, effective_dir: None, worktree_branch: None,
        final_head_sha: None, final_branch: None, start_sha: None, log_path: None, output_path: None,
        tokens: None, prompt_tokens: None, duration_ms: None, requested_model: None,
        observed_model: None, attribution_source: None, cost_usd: None, exit_code: None,
        created_at: Local::now(), completed_at: None, verify: None, verify_status: VerifyStatus::Skipped,
        pending_reason: None, read_only: false, budget: false, audit_verdict: None,
        audit_report_path: None, delivery_assessment: None,
    }
}

#[test]
fn tui_default_scope_keeps_every_project_and_unattributed_task() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut alpha = task("t-project-alpha"); alpha.project_id = Some("project-alpha".into());
    let mut beta = task("t-project-beta"); beta.project_id = Some("project-beta".into());
    store.insert_task(&alpha).unwrap(); store.insert_task(&beta).unwrap();
    store.insert_task(&task("t-project-unattributed")).unwrap();
    let app = App::new(store, super::super::RunOptions::default()).unwrap();
    let mut ids: Vec<&str> = app.tasks.iter().map(|value| value.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["t-project-alpha", "t-project-beta", "t-project-unattributed"]);
}

#[test]
fn grouped_navigation_keeps_jk_in_group_and_hl_between_groups() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut alpha = task("t-nav-alpha"); alpha.project_id = Some("project-alpha".into());
    let mut beta = task("t-nav-beta"); beta.project_id = Some("project-beta".into());
    store.insert_task(&alpha).unwrap(); store.insert_task(&beta).unwrap();
    let mut app = App::new(store, super::super::RunOptions::default()).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)).unwrap();
    let first = crate::tui::tree_data::build_task_tree_with_state(
        &app.tasks, &app.wg_creators, &app.collapsed_projects,
    )[app.tree_selected].project_id.clone();
    assert_eq!(app.selected_task().and_then(|value| value.project_id.clone()), first);
    app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)).unwrap();
    let node = &crate::tui::tree_data::build_task_tree_with_state(
        &app.tasks, &app.wg_creators, &app.collapsed_projects,
    )[app.tree_selected];
    assert!(node.is_group_header); assert_ne!(node.project_id, first);
    let collapsed = node.project_id.clone();
    app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)).unwrap();
    assert_eq!(crate::tui::tree_data::build_task_tree_with_state(
        &app.tasks, &app.wg_creators, &app.collapsed_projects,
    ).iter().filter(|value| value.project_id == collapsed).count(), 1);
}

#[test]
fn visible_navigation_reaches_every_node_across_collapsed_group() {
    let store = Arc::new(Store::open_memory().unwrap());
    for (id, project_id, age) in [
        ("t-nav-old", "project-old", 30),
        ("t-nav-middle", "project-middle", 20),
        ("t-nav-new", "project-new", 10),
    ] {
        let mut value = task(id);
        value.project_id = Some(project_id.into());
        value.created_at = Local::now() - Duration::seconds(age);
        store.insert_task(&value).unwrap();
    }
    let mut app = App::new(store, super::super::RunOptions::default()).unwrap();
    app.collapsed_projects.insert(Some("project-middle".into()));
    app.tree_selected = 0;

    let nodes = crate::tui::tree_data::build_task_tree_with_state(
        &app.tasks, &app.wg_creators, &app.collapsed_projects,
    );
    let collapsed_index = nodes
        .iter()
        .position(|node| node.is_group_header && node.project_id.as_deref() == Some("project-middle"))
        .expect("collapsed group header is visible");
    assert!(collapsed_index > 0 && collapsed_index + 1 < nodes.len());
    let mut visited = std::collections::HashSet::from([app.tree_selected]);
    for _ in 0..nodes.len() {
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)).unwrap();
        visited.insert(app.tree_selected);
    }

    let expected: std::collections::HashSet<usize> = (0..nodes.len()).collect();
    assert_eq!(visited, expected);
}

#[test]
fn slash_search_selects_a_matching_task_without_database_queries() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut matching = task("t-find-me"); matching.prompt = "find this exact task".into();
    store.insert_task(&matching).unwrap();
    let mut app = App::new(store, super::super::RunOptions::default()).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)).unwrap();
    for character in "exact task".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap();
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
    assert!(!app.search_mode);
    assert_eq!(app.selected_task().map(|value| value.id.as_str()), Some("t-find-me"));
}

#[test]
fn search_mode_uses_n_and_n_for_next_previous_and_escape_cancels() {
    let store = Arc::new(Store::open_memory().unwrap());
    let first = task("t-find-first");
    let second = task("t-find-second");
    store.insert_task(&first).unwrap();
    store.insert_task(&second).unwrap();
    let mut app = App::new(store, super::super::RunOptions::default()).unwrap();
    app.selected = app.tasks.iter().position(|value| value.id == first.id).unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)).unwrap();
    for character in "prompt".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap();
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)).unwrap();
    assert_eq!(app.selected_task().map(|value| value.id.as_str()), Some("t-find-second"));
    app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE)).unwrap();
    assert_eq!(app.selected_task().map(|value| value.id.as_str()), Some("t-find-first"));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).unwrap();
    assert!(!app.search_mode);
}

#[test]
fn animation_only_changes_a_real_reasoning_state() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut reasoning = task("t-thinking");
    reasoning.status = TaskStatus::Running;
    store.insert_task(&reasoning).unwrap();
    store.insert_event(&TaskEvent {
        task_id: reasoning.id.clone(), timestamp: Local::now(), event_kind: EventKind::Reasoning,
        detail: "checking".into(), metadata: None,
    }).unwrap();
    let mut app = App::new(store, super::super::RunOptions::default()).unwrap();
    let first = app.task_activity(&reasoning);
    app.tick().unwrap();
    let second = app.task_activity(&reasoning);
    assert!(first.starts_with("THINKING · reasoning ·"));
    assert_ne!(first, second);

    let mut ordinary = reasoning;
    ordinary.id = TaskId("t-ordinary".into());
    ordinary.status = TaskStatus::Running;
    assert!(!app.task_activity(&ordinary).starts_with("THINKING ·"));
}
