// Manual TUI performance investigation against an explicit read-only DB snapshot.
// Exports one ignored diagnostic test; measures real Store/App/render code without a terminal.
// Deps: ratatui TestBackend, isolated AID_HOME, Store, std timers.

use std::sync::Arc;
use std::time::{Duration, Instant};
use ratatui::{Terminal, backend::TestBackend};
use crate::store::Store;
use crate::types::TaskFilter;
use super::{RunOptions, app::App};

#[test]
#[ignore = "set AID_TUI_PROFILE_DB to a read-only snapshot; manual performance probe"]
fn profile_tui_snapshot() {
    let path = std::env::var("AID_TUI_PROFILE_DB").expect("explicit snapshot path required");
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_read_only(std::path::Path::new(&path)).unwrap().unwrap());
    crate::store::optimize_for_concurrency(&store.db()).unwrap();
    let mut app = measure("app_new", || App::new(store.clone(), RunOptions::default()).unwrap());
    println!("PROFILE visible_tasks={}", app.tasks.len());
    let ids: Vec<&str> = app.tasks.iter().map(|task| task.id.as_str()).collect();
    for _ in 0..3 {
        measure("latest_events", || store.latest_events_batch(&ids).unwrap());
        measure("candidate_latest_events", || candidate_events(&store, &ids));
        measure("milestones", || store.latest_milestones_batch(&ids).unwrap());
        measure("workgroups", || store.list_workgroups().unwrap());
    }
    measure("list_today", || store.list_tasks(TaskFilter::Today).unwrap());
    measure("list_active", || store.list_tasks(TaskFilter::Active).unwrap());
    for _ in 0..3 { measure_render(&app); }
    std::thread::sleep(Duration::from_millis(2100));
    measure("due_tick", || app.tick().unwrap());
    let task = app.tasks.iter().max_by_key(|task| store.get_events(task.id.as_str()).unwrap().len()).unwrap();
    let events = measure("largest_visible_event_history", || store.get_events(task.id.as_str()).unwrap());
    println!("PROFILE largest_visible_event_count={}", events.len());
    app.tasks = measure("list_all", || store.list_tasks(TaskFilter::All).unwrap());
    println!("PROFILE all_tasks={}", app.tasks.len());
    app.nodes = super::tree_data::build_task_tree_with_state(&app.tasks, &app.wg_creators, &app.collapsed_projects).into();
    measure_render(&app);
}

fn measure<T>(label: &str, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = run();
    println!("PROFILE {label}={:.3}ms", start.elapsed().as_secs_f64() * 1000.0);
    value
}

fn measure_render(app: &App) {
    measure("build_tree", || super::tree_data::build_task_tree_with_state(
        &app.tasks, &app.wg_creators, &app.collapsed_projects,
    ));
    let mut terminal = Terminal::new(TestBackend::new(220, 50)).unwrap();
    measure("render_220x50", || { terminal.draw(|frame| super::ui::render(frame, app)).unwrap(); });
}

fn candidate_events(store: &Store, ids: &[&str]) -> usize {
    if ids.is_empty() { return 0; }
    let values = vec!["(?)"; ids.len()].join(",");
    let sql = format!(
        "WITH requested(task_id) AS (VALUES {values})
         SELECT e.task_id, e.timestamp, e.event_type, e.detail, e.metadata
         FROM requested r JOIN events e ON e.id = (
             SELECT id FROM events WHERE task_id = r.task_id
             ORDER BY timestamp DESC, id DESC LIMIT 1
         )"
    );
    let connection = store.db();
    let mut statement = connection.prepare(&sql).unwrap();
    let rows = statement.query_map(rusqlite::params_from_iter(ids), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
            row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?))
    }).unwrap();
    rows.map(|row| row.unwrap()).count()
}
