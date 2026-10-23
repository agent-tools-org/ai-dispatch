// Narrow event reads for completion pricing, board diagnostics, waiting prompts, and liveness.
// Exports Store event selectors; depends on existing event indexes and guarded JSON expressions.

use super::*;
use serde_json::Value;

const COMPLETION_SQL: &str = "SELECT metadata FROM events INDEXED BY idx_events_task_kind
    WHERE task_id = ?1 AND event_type = 'completion' ORDER BY timestamp DESC, id DESC";

const ACTIVITY_WHERE: &str = "task_id = ?1 AND event_type NOT IN ('noop', 'setup')
    AND NOT ((event_type = 'reasoning' OR event_type NOT IN (
        'tool_call', 'milestone', 'build', 'test', 'commit', 'completion', 'error',
        'file_write', 'file_read', 'web_search', 'lint', 'format')) AND trim(detail, ?2) = ?3)
    AND NOT EXISTS (
        SELECT 1 FROM (
            SELECT key, type, value, ROW_NUMBER() OVER (PARTITION BY key ORDER BY id DESC) AS ordinal
            FROM json_each(CASE WHEN json_valid(metadata) THEN metadata END)
            WHERE key IN ('idle_warn', 'auto_escalated', 'acked_reply', 'steered',
                          'source', 'input_delivery', 'message_id')
        ) WHERE ordinal = 1 AND (
            (key IN ('idle_warn', 'auto_escalated', 'acked_reply', 'steered') AND type = 'true')
            OR (key = 'source' AND type = 'text' AND value = 'unstick-auto')
            OR key IN ('input_delivery', 'message_id')
        )
    )";

impl Store {
    pub(crate) fn latest_usage_metadata(&self, task_id: &str) -> Result<Option<Value>> {
        let conn = self.db();
        let mut stmt = conn.prepare(COMPLETION_SQL)?;
        let mut rows = stmt.query(params![task_id])?;
        while let Some(row) = rows.next()? {
            let raw: Option<String> = row.get(0)?;
            let metadata = raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
            if let Some(metadata) = metadata
                && (metadata.get("tokens").is_some() || metadata.get("cost_usd").is_some())
            {
                return Ok(Some(metadata));
            }
        }
        Ok(None)
    }

    pub(crate) fn latest_waiting_prompt(&self, task_id: &str) -> Result<Option<String>> {
        self.latest_metadata_text(task_id, "awaiting_prompt", "text")
    }

    pub(crate) fn latest_infrastructure_error(&self, task_id: &str) -> Result<Option<String>> {
        self.latest_metadata_text(task_id, "verify_execution_error", "true")
    }

    fn latest_metadata_text(&self, task_id: &str, key: &str, kind: &str) -> Result<Option<String>> {
        let conn = self.db();
        let detail = if kind == "true" { "detail" } else { "NULL" };
        let sql = format!(
            "SELECT {detail}, metadata FROM events WHERE task_id = ?1
             AND json_type(CASE WHEN json_valid(metadata) THEN metadata END, ?2) IS NOT NULL
             ORDER BY timestamp DESC, id DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![task_id, format!("$.{key}")])?;
        while let Some(row) = rows.next()? {
            let raw: String = row.get(1)?;
            let Some(metadata) = serde_json::from_str::<Value>(&raw).ok() else {
                continue;
            };
            let value = metadata.get(key);
            if kind == "true" && value.and_then(Value::as_bool) == Some(true) {
                return Ok(Some(row.get(0)?));
            }
            if kind == "text"
                && let Some(value) = value.and_then(Value::as_str)
            {
                return Ok(Some(value.to_string()));
            }
        }
        Ok(None)
    }

    pub(crate) fn liveness_summary(
        &self,
        task_id: &str,
    ) -> Result<(u32, Option<(chrono::DateTime<chrono::Local>, String)>)> {
        let whitespace: String = (0..=0x3000)
            .filter_map(char::from_u32)
            .filter(|c| c.is_whitespace())
            .collect();
        let nudge = crate::task::unstick::default_nudge_message();
        let conn = self.db();
        let snapshot = conn.unchecked_transaction()?;
        let count: u32 = snapshot.query_row(
            &format!("SELECT COUNT(*) FROM events WHERE {ACTIVITY_WHERE}"),
            params![task_id, whitespace, nudge],
            |row| row.get(0),
        )?;
        let latest = snapshot
            .query_row(
                &format!(
                    "SELECT timestamp, detail FROM events WHERE {ACTIVITY_WHERE}
                      ORDER BY timestamp DESC, id DESC LIMIT 1"
                ),
                params![task_id, whitespace, nudge],
                |row| {
                    Ok((
                        super::super::super::schema::parse_dt(&row.get::<_, String>(0)?),
                        row.get(1)?,
                    ))
                },
            )
            .optional()?;
        Ok((count, latest))
    }
}

#[cfg(test)]
#[path = "event_reads_tests.rs"]
mod tests;
