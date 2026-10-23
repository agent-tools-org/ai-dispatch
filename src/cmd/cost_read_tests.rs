// Cost rollup equivalence and filter-before-pricing regressions on noisy histories.
// Compares command report helpers against the old full task read.

use super::*;
use crate::types::TaskFilter;

fn seed(store: &Store) {
    let now = Local::now();
    for i in 0..150 {
        let created = if i < 75 {
            now
        } else {
            now - chrono::Duration::days(90)
        };
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, resolved_prompt, status, created_at,
             duration_ms, tokens, cost_usd, workgroup_id)
             VALUES (?1, ?2, ?3, ?3, 'done', ?4, 1000, 100, 1, ?5)",
                rusqlite::params![
                    format!("t-{i}"),
                    if i % 2 == 0 { "cursor" } else { "copilot" },
                    "prompt".repeat(1000),
                    created.to_rfc3339(),
                    if i % 3 == 0 { "selected" } else { "other" }
                ],
            )
            .unwrap();
    }
}

fn price(store: &Store, tasks: &mut [Task]) {
    for task in tasks {
        task.cost_usd = cost::task_cost(store, task).unwrap();
    }
}

#[test]
fn cost_filtered_sql_keeps_group_agent_and_summary_output() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    let now = Local::now();
    let mut all = store.list_tasks(TaskFilter::All).unwrap();
    price(&store, &mut all);
    let mut group = store.read_report_tasks(Some("selected"), None, None).unwrap();
    price(&store, &mut group);
    assert_eq!(
        render_task_report("group", &group_tasks(&all, "selected")),
        render_task_report("group", &group.iter().collect::<Vec<_>>())
    );
    for window in [UsageWindow::All, UsageWindow::Today, UsageWindow::Days(7)] {
        for agent in [None, Some("CURSOR"), Some("missing")] {
            let mut actual = store.read_report_tasks(None, agent, window.range(now)).unwrap();
            price(&store, &mut actual);
            if let Some(agent) = agent {
                assert_eq!(
                    render_task_report("agent", &agent_tasks(&all, agent, window, now)),
                    render_task_report("agent", &actual.iter().collect::<Vec<_>>())
                );
            } else {
                assert_eq!(
                    render_daily_report(&daily_summary_rows(&all, window, now)),
                    render_daily_report(&daily_summary_rows(&actual, window, now))
                );
            }
        }
    }
}

#[test]
fn cost_does_not_price_tasks_excluded_by_group() {
    let store = Arc::new(Store::open_memory().unwrap());
    seed(&store);
    store
        .db()
        .execute(
            "UPDATE tasks SET agent = 'codex' WHERE workgroup_id = 'other'",
            [],
        )
        .unwrap();
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         SELECT id, created_at, 'completion', 'complete', X'ff' FROM tasks WHERE workgroup_id = 'other'",
            [],
        )
        .unwrap();
    let all = store.list_tasks(TaskFilter::All).unwrap();
    assert!(all.iter().any(|task| cost::task_cost(&store, task).is_err()));
    run(&store, Some("selected".into()), false, None, "today".into()).unwrap();
}
