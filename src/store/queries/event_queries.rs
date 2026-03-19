// Event-related store query methods.
// Exports: Store event lookup methods.
// Deps: super::super::Store, rusqlite, chrono.

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::super::schema::row_to_event;
use super::super::Store;
use crate::types::TaskEvent;

impl Store {
    pub fn latest_error(&self, task_id: &str) -> Option<String> {
        let conn = self.db();
        conn.query_row(
            "SELECT detail FROM events
             WHERE task_id = ?1 AND event_type = 'error'
             ORDER BY timestamp DESC
             LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn latest_milestone(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.db();
        let milestone = conn
            .query_row(
                "SELECT detail FROM events
                 WHERE task_id = ?1 AND event_type = 'milestone'
                 ORDER BY timestamp DESC
                 LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(milestone)
    }

    pub fn latest_milestones_batch(&self, task_ids: &[&str]) -> Result<HashMap<String, String>> {
        if task_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.db();
        let placeholders: Vec<String> = (1..=task_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT task_id, detail FROM events e1
             WHERE event_type = 'milestone'
             AND timestamp = (
                 SELECT MAX(timestamp) FROM events e2
                 WHERE e2.task_id = e1.task_id AND e2.event_type = 'milestone'
             )
             AND task_id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            task_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (task_id, detail) = row?;
            map.insert(task_id, detail);
        }
        Ok(map)
    }

    pub fn latest_awaiting_reasons_batch(&self, task_ids: &[&str]) -> Result<HashMap<String, String>> {
        if task_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.db();
        let placeholders: Vec<String> = (1..=task_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT task_id, json_extract(metadata, '$.awaiting_prompt') FROM events e1
             WHERE json_extract(metadata, '$.awaiting_input') = 1
             AND timestamp = (
                 SELECT MAX(timestamp) FROM events e2
                 WHERE e2.task_id = e1.task_id
                   AND json_extract(e2.metadata, '$.awaiting_input') = 1
             )
             AND task_id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            task_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (task_id, prompt) = row?;
            if let Some(prompt) = prompt {
                map.insert(task_id, prompt);
            }
        }
        Ok(map)
    }

    pub fn get_workgroup_milestones(&self, workgroup_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT e.task_id, e.detail FROM events e
             JOIN tasks t ON e.task_id = t.id
             WHERE t.workgroup_id = ?1 AND e.event_type = 'milestone'
             ORDER BY e.timestamp ASC",
        )?;
        let rows = stmt.query_map(params![workgroup_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn get_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT task_id, timestamp, event_type, detail, metadata
             FROM events WHERE task_id = ?1 ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![task_id], row_to_event)?;
        rows.map(|row| Ok(row?)).collect()
    }
}
