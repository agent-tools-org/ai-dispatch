// Usage rows retain named API account identity alongside plain Claude.
// Reads synthetic tasks through the real store before collecting usage rows.
use super::*;
use crate::store::Store;
use crate::types::TaskFilter;

#[test]
fn usage_groups_api_accounts_separately() {
    let store = Store::open_memory().unwrap();
    for (id, agent, name, cost) in [
        ("t-ming", "custom", Some("claude-api-ming"), 37.2),
        ("t-meet", "custom", Some("claude-api-meet"), 12.0),
        ("t-claude", "claude", None, 8.0),
    ] {
        store
            .insert_waiting_task(
                id, "claude", "test", None, None, None, None, None, None, false, false,
            )
            .unwrap();
        store
            .db()
            .execute(
                "UPDATE tasks SET agent = ?1, custom_agent_name = ?2,
            cost_usd = ?3 WHERE id = ?4",
                rusqlite::params![agent, name, cost, id],
            )
            .unwrap();
    }
    let rows = collect_agent_rows(&store.list_tasks(TaskFilter::All).unwrap());
    assert_eq!(rows.len(), 3);
    for (name, cost) in [
        ("claude-api-ming", 37.2),
        ("claude-api-meet", 12.0),
        ("claude", 8.0),
    ] {
        assert_eq!(rows.iter().find(|row| row.name == name).unwrap().cost_usd, cost);
    }
}
