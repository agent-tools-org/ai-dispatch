// Budget usage aggregation by agent, project, and time window.
// Deps: parent query test fixtures.

use super::*;

#[test]
fn aggregates_budget_usage_by_agent_and_window() {
    let store = Store::open_memory().unwrap();
    let conn = store.db();
    let now = Local::now();
    let within_window = (now - Duration::hours(6)).to_rfc3339();
    let outside_window = (now - Duration::days(2)).to_rfc3339();

    for (id, agent, prompt, repo, tokens, cost, created) in [
        (
            "t-recent-1",
            "codex",
            "recent",
            "/work/project-a",
            120_i64,
            1.25_f64,
            &within_window,
        ),
        (
            "t-recent-2",
            "codex",
            "recent",
            "/work/project-a",
            80,
            0.75,
            &within_window,
        ),
        (
            "t-old",
            "codex",
            "old",
            "/work/project-a",
            500,
            4.0,
            &outside_window,
        ),
        (
            "t-other",
            "grok",
            "other",
            "/work/project-b",
            999,
            9.0,
            &within_window,
        ),
    ] {
        conn.execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, tokens, cost_usd, created_at)
             VALUES (?1, ?2, ?3, 'done', ?4, ?5, ?6, ?7)",
            params![id, agent, prompt, repo, tokens, cost, created],
        )
        .unwrap();
    }
    conn.execute(
        "UPDATE tasks SET project_id = 'project-a' WHERE repo_path = '/work/project-a'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET project_id = 'project-b' WHERE repo_path = '/work/project-b'",
        [],
    )
    .unwrap();
    drop(conn);

    let all_time = store.budget_usage_summary("codex", None).unwrap();
    assert_eq!(all_time, (3, 700, 6.0));

    let since = parse_window("24h").map(|window| now - window).unwrap();
    let recent = store.budget_usage_summary("codex", Some(since)).unwrap();
    assert_eq!(recent, (2, 200, 2.0));

    let all_agents_recent = store.budget_usage_summary_for_agent(None, Some(since)).unwrap();
    assert_eq!(all_agents_recent, (3, 1199, 11.0));

    let project_recent = store
        .budget_usage_summary_for_project("project-a", Some(since))
        .unwrap();
    assert_eq!(project_recent, (2, 200, 2.0));
}
