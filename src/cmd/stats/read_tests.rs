// Stats output equivalence with SQL period/agent filtering and tied task timestamps.
// Compares the existing aggregation and rendering against the old full task read.

use super::*;

#[test]
fn stats_filtered_sql_preserves_the_complete_rendered_report() {
    let store = Store::open_memory().unwrap();
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
             duration_ms, tokens, cost_usd)
             VALUES (?1, ?2, ?3, ?3, 'done', ?4, 1000, 100, 0)",
                rusqlite::params![
                    format!("t-{i}"),
                    if i % 2 == 0 { "cursor" } else { "copilot" },
                    "prompt".repeat(1000),
                    created.to_rfc3339()
                ],
            )
            .unwrap();
    }
    for window in [UsageWindow::All, UsageWindow::Today, UsageWindow::Days(7)] {
        for agent in [None, Some("CURSOR"), Some("missing")] {
            let tasks = store
                .list_tasks(crate::types::TaskFilter::All)
                .unwrap()
                .into_iter()
                .filter(|task| {
                    window
                        .range(now)
                        .is_none_or(|(start, end)| task.created_at >= start && task.created_at <= end)
                })
                .filter(|task| agent.is_none_or(|name| task.agent_display_name().eq_ignore_ascii_case(name)))
                .collect::<Vec<_>>();
            let mut run = collection::StatsRun::new();
            run.aggregate(&store, &tasks).unwrap();
            let expected = run.finish().unwrap();
            let actual = collect(&store, window, agent, now).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                render_output(&actual, window, true, agent.is_some()),
                render_output(&expected, window, true, agent.is_some())
            );
        }
    }
}
