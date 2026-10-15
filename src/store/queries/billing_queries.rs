// Indexed dispatch billing evidence for usage reporting.
// Returns task IDs only; task creation dates match existing usage period semantics.

use super::super::Store;
use chrono::{DateTime, Local};
use rusqlite::params;
use std::collections::HashSet;

const OVERAGE_TASK_IDS_SQL: &str = "SELECT DISTINCT e.task_id
    FROM events e INDEXED BY idx_events_overage
    CROSS JOIN tasks t ON t.id = e.task_id
    WHERE e.event_type = 'setup'
      AND json_extract(CASE WHEN json_valid(e.metadata) THEN e.metadata END,
                       '$.billing_state') = 'overage'
      AND (?1 IS NULL OR julianday(t.created_at) >= julianday(?1))
      AND (?2 IS NULL OR julianday(t.created_at) <= julianday(?2))";

impl Store {
    pub(crate) fn overage_task_ids(
        &self,
        range: Option<(DateTime<Local>, DateTime<Local>)>,
    ) -> anyhow::Result<HashSet<String>> {
        let (start, end) = match range {
            Some((start, end)) => (Some(start.to_rfc3339()), Some(end.to_rfc3339())),
            None => (None, None),
        };
        let conn = self.db();
        let mut stmt = conn.prepare(OVERAGE_TASK_IDS_SQL)?;
        let rows = stmt.query_map(params![start, end], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
#[path = "billing_queries_tests.rs"]
mod tests;
