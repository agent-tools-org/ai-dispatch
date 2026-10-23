// Narrow event reads for completion pricing, board diagnostics, waiting prompts, and liveness.
// Exports Store event selectors; depends on existing event indexes and serde metadata parsing.

use super::*;
use serde_json::Value;

const COMPLETION_SQL: &str = "SELECT metadata FROM events INDEXED BY idx_events_task_kind
    WHERE task_id = ?1 AND event_type = 'completion' ORDER BY id DESC";

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
        // These ASCII keys can only be escaped with \u sequences in valid JSON.
        let sql = format!(
            "SELECT {detail}, metadata FROM events WHERE task_id = ?1
             AND metadata IS NOT NULL AND (instr(metadata, ?2) > 0 OR instr(metadata, char(92) || 'u') > 0)
             ORDER BY timestamp DESC, id DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![task_id, key])?;
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
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT task_id, timestamp, event_type, detail, metadata FROM events
             WHERE task_id = ?1 AND event_type NOT IN ('noop', 'setup')
             ORDER BY timestamp ASC, id ASC",
        )?;
        let mut rows = stmt.query(params![task_id])?;
        let mut count = 0_u32;
        let mut latest = None;
        while let Some(row) = rows.next()? {
            let event = row_to_event(row)?;
            if is_progress_event(&event) {
                count = count.wrapping_add(1);
                latest = Some((event.timestamp, event.detail));
            }
        }
        Ok((count, latest))
    }
}

fn is_progress_event(event: &crate::types::TaskEvent) -> bool {
    use crate::types::EventKind;
    if !event.event_kind.is_liveness() || event.event_kind == EventKind::Setup {
        return false;
    }
    if event.event_kind == EventKind::Reasoning
        && event.detail.trim() == crate::task::unstick::default_nudge_message()
    {
        return false;
    }
    let Some(metadata) = event.metadata.as_ref() else {
        return true;
    };
    !["idle_warn", "auto_escalated", "acked_reply", "steered"]
        .iter()
        .any(|key| metadata.get(key).and_then(Value::as_bool) == Some(true))
        && metadata.get("source").and_then(Value::as_str) != Some("unstick-auto")
        && metadata.get("input_delivery").is_none()
        && metadata.get("message_id").is_none()
}

#[cfg(test)]
#[path = "event_reads_tests.rs"]
mod tests;
