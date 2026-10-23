// API-route launch lookup through the existing task and event-kind index.
// Tasks drive indexed setup-event searches without scanning other event kinds.
use super::Store;
use anyhow::Result;

const LOOKUP: &str = "SELECT DISTINCT e.task_id FROM tasks t
    CROSS JOIN events e INDEXED BY idx_events_task_kind
      ON e.task_id = t.id AND e.event_type = 'setup'
    WHERE metadata LIKE '%api_route_launch%'
      AND json_extract(CASE WHEN json_valid(metadata) THEN metadata END,
                       '$.api_route_launch') = ?1";

pub(super) fn lookup(store: &Store, route: &str) -> Result<Vec<String>> {
    let conn = store.db();
    let mut stmt = conn.prepare(LOOKUP)?;
    let rows = stmt.query_map([route], |row| row.get(0))?;
    rows.map(|row| Ok(row?)).collect()
}

#[cfg(test)]
#[path = "route_launch_tests.rs"]
mod tests;
