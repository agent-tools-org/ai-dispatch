// Background refresh integration tests with a deliberately blocked database.
// Verifies responsive input, coalescing, stale-scope rejection and visible event bounds.
// Deps: App, Store, ratatui TestBackend, std channels and deadlines.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

fn key(app: &mut App, value: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(value), KeyModifiers::NONE)).unwrap();
}

fn settle(worker: &mut RefreshWorker, app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker.in_flight || app.refresh_requested {
        worker.poll(app).unwrap();
        assert!(Instant::now() < deadline, "refresh did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn blocked_refresh_keeps_input_and_render_responsive_and_coalesces_scope() {
    let store = Arc::new(crate::store::Store::open_memory().unwrap());
    let mut task = super::super::tests::make_task("t-old", None);
    task.created_at = chrono::Local::now() - chrono::Duration::days(10);
    task.status = crate::types::TaskStatus::Done;
    store.insert_task(&task).unwrap();
    let mut app = App::empty(store.clone(), crate::tui::RunOptions::default());
    let mut worker = RefreshWorker::start(&app).unwrap();
    let database_lock = store.db();
    let start = Instant::now();
    worker.poll(&mut app).unwrap();
    for _ in 0..20 { key(&mut app, 'a'); worker.poll(&mut app).unwrap(); }
    key(&mut app, 'a');
    key(&mut app, 'q');
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| crate::tui::ui::render(frame, &app)).unwrap();
    assert!(app.should_quit);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(worker.in_flight);
    drop(database_lock);
    settle(&mut worker, &mut app);
    assert!(app.show_all);
    assert_eq!(app.tasks.len(), 1);
    assert_eq!(app.tasks[0].id, task.id);
    assert!(app.refresh_status.is_none());
}

#[test]
fn multipane_snapshot_fetches_only_six_visible_histories() {
    let store = Arc::new(crate::store::Store::open_memory().unwrap());
    for index in 0..12 {
        store.insert_task(&super::super::tests::make_task(&format!("t-{index}"), None)).unwrap();
    }
    let mut app = App::empty(store, crate::tui::RunOptions::default());
    key(&mut app, 'm');
    let mut worker = RefreshWorker::start(&app).unwrap();
    settle(&mut worker, &mut app);
    assert_eq!(app.tasks.len(), 12);
    assert_eq!(app.events_cache.len(), 6);
}

#[test]
fn refresh_preserves_selection_and_reports_failure_without_losing_rows() {
    let store = Arc::new(crate::store::Store::open_memory().unwrap());
    let mut task = super::super::tests::make_task("t-selected", None);
    task.created_at = chrono::Local::now() - chrono::Duration::seconds(30);
    store.insert_task(&task).unwrap();
    let mut app = App::empty(store.clone(), crate::tui::RunOptions::default());
    let mut worker = RefreshWorker::start(&app).unwrap();
    settle(&mut worker, &mut app);
    key(&mut app, 'j');
    store.insert_task(&super::super::tests::make_task("t-newer", None)).unwrap();
    key(&mut app, 'r');
    settle(&mut worker, &mut app);
    assert_eq!(app.selected_task().unwrap().id, task.id);
    assert_eq!(app.nodes[app.tree_selected].task_id, task.id);
    store.db().execute_batch("DROP TABLE events;").unwrap();
    key(&mut app, 'r');
    settle(&mut worker, &mut app);
    assert_eq!(app.tasks.len(), 2);
    assert_eq!(app.selected_task().unwrap().id, task.id);
    assert!(app.refresh_status.as_deref().unwrap().starts_with("Refresh failed:"));
}
