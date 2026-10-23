// Regression tests for SQL task filtering, ordering, payload projection, and limits.
// Compares narrow reads against the original list-and-retain behavior on tied timestamps.

use super::*;

fn seed(store: &Store) {
    let conn = store.db();
    for i in 0..160 {
        conn.execute(
            "INSERT INTO tasks (id, agent, prompt, resolved_prompt, status, created_at,
             workgroup_id, project_id, caller_kind, caller_session_id)
             VALUES (?1, 'codex', ?2, ?2, ?3, ?4, ?5, ?6, 'codex', ?7)",
            params![
                format!("t-{i}"),
                "prompt".repeat(1000),
                ["done", "running", "awaiting_input", "stalled"][i % 4],
                if i < 80 {
                    Local::now().to_rfc3339()
                } else {
                    "2020-01-01T00:00:00Z".into()
                },
                if i % 3 == 0 { "group" } else { "other" },
                if i % 2 == 0 { Some("project") } else { None },
                if i % 2 == 0 { "session" } else { "other" },
            ],
        )
        .unwrap();
    }
    conn.execute(
        "UPDATE tasks SET created_at = ?1 WHERE id IN ('t-0', 't-1', 't-2', 't-3')",
        params![Local::now().to_rfc3339()],
    )
    .unwrap();
}

fn ids(tasks: &[Task]) -> Vec<&str> {
    tasks.iter().map(|task| task.id.as_str()).collect()
}

#[test]
fn board_sql_matches_filters_limits_and_timestamp_ties() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    let caller = crate::runtime::session::CallerSession {
        kind: "codex".into(),
        session_id: "session".into(),
    };
    for filter in [
        TaskFilter::All,
        TaskFilter::Today,
        TaskFilter::Running,
        TaskFilter::Active,
    ] {
        for project in [None, Some(None), Some(Some("project"))] {
            for group in [None, Some("group")] {
                for mine in [None, Some(None), Some(Some(&caller))] {
                    let scope = TaskReadFilter {
                        group,
                        project,
                        caller: mine,
                    };
                    let mut expected = store.list_tasks(filter).unwrap();
                    expected.retain(|task| {
                        group.is_none_or(|group| task.workgroup_id.as_deref() == Some(group))
                            && project.is_none_or(|project| task.project_id.as_deref() == project)
                            && mine.is_none_or(|caller| {
                                caller.is_some_and(|caller| {
                                    task.caller_kind.as_deref() == Some(&caller.kind)
                                        && task.caller_session_id.as_deref() == Some(&caller.session_id)
                                })
                            })
                    });
                    for limit in [None, Some(0), Some(2), Some(500)] {
                        let (actual, total) = store.read_board_tasks(filter, &scope, limit).unwrap();
                        assert_eq!(total, expected.len());
                        assert_eq!(ids(&actual), ids(&expected[..limit.unwrap_or(total).min(total)]));
                        assert!(actual.iter().all(|task| task.resolved_prompt.is_none()));
                    }
                }
            }
        }
    }
}

#[test]
fn board_sql_does_not_decode_rows_past_limit() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    store
        .db()
        .execute("UPDATE tasks SET prompt = X'ff' WHERE id = 't-159'", [])
        .unwrap();
    assert!(store.list_tasks(TaskFilter::All).is_err());
    let (tasks, count) = store
        .read_board_tasks(TaskFilter::All, &TaskReadFilter::default(), Some(2))
        .unwrap();
    assert_eq!((tasks.len(), count), (2, 160));
}

#[test]
fn board_limit_selects_row_ids_before_task_payloads() {
    let store = Store::open_memory().unwrap();
    let (base, mut values) = board_query(TaskFilter::All, &TaskReadFilter::default());
    let conn = store.db();
    let query = limited_board_query(base, TaskFilter::All, &mut values, Some(2)).unwrap();
    let sql = format!("EXPLAIN {query}");
    let mut stmt = conn.prepare(&sql).unwrap();
    let opcodes = stmt
        .query_map(rusqlite::params_from_iter(values), |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(opcodes.iter().any(|op| op == "DecrJumpZero"));
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {query}")).unwrap();
    let plan = stmt
        .query_map(rusqlite::params_from_iter(["2"]), |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("INTEGER PRIMARY KEY"), "{plan}");
    assert!(plan.contains("idx_tasks_created_at"), "{plan}");
}

fn seed_report_boundaries(store: &Store, start: DateTime<Local>, end: DateTime<Local>) {
    for (id, agent, custom) in [
        ("t-0", "antigravity", None),
        ("t-1", "new-agent", None),
        ("t-2", "custom", Some("aGent")),
        ("t-3", "CODEX", Some("ignored")),
    ] {
        store
            .db()
            .execute(
                "UPDATE tasks SET agent = ?1, custom_agent_name = ?2 WHERE id = ?3",
                params![agent, custom, id],
            )
            .unwrap();
    }
    for (id, date) in [
        ("t-4", start),
        ("t-5", end),
        ("t-6", start - chrono::Duration::nanoseconds(1)),
    ] {
        store
            .db()
            .execute(
                "UPDATE tasks SET created_at = ?1 WHERE id = ?2",
                params![date.to_rfc3339(), id],
            )
            .unwrap();
    }
}

#[test]
fn report_sql_preserves_agent_aliases_custom_names_and_exact_time_boundaries() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    let end = Local::now();
    let start = end - chrono::Duration::days(7);
    seed_report_boundaries(&store, start, end);
    for agent in [
        None,
        Some("AGY"),
        Some("new-agent"),
        Some("AGENT"),
        Some("Codex"),
        Some("ignored"),
    ] {
        let expected = store
            .list_tasks(TaskFilter::All)
            .unwrap()
            .into_iter()
            .filter(|task| {
                agent.is_none_or(|name| task.agent_display_name().eq_ignore_ascii_case(name))
                    && task.created_at >= start
                    && task.created_at <= end
            })
            .collect::<Vec<_>>();
        let actual = store.read_report_tasks(None, agent, Some((start, end))).unwrap();
        assert_eq!(ids(&actual), ids(&expected));
        assert!(
            actual
                .iter()
                .all(|task| task.prompt.is_empty() && task.resolved_prompt.is_none())
        );
    }
}
