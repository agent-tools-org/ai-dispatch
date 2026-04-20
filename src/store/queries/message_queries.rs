// Message-related store query methods for task reply delivery state.
// Exports: Store task message insert/list/pending/delivery/ack methods.
// Deps: super::super::Store, rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::Local;
use rusqlite::types::Type;
use rusqlite::{Row, params};

use super::super::Store;
use super::super::schema::parse_dt;
use crate::types::{MessageDirection, MessageSource, TaskId, TaskMessage};

const MESSAGE_COLUMNS: &str =
    "id, task_id, direction, content, source, created_at, delivered_at, acked_at";

impl Store {
    pub fn insert_message(
        &self,
        task_id: &str,
        direction: MessageDirection,
        content: &str,
        source: MessageSource,
    ) -> Result<TaskMessage> {
        let created_at = Local::now();
        let conn = self.db();
        conn.execute(
            "INSERT INTO task_messages (task_id, direction, content, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task_id,
                direction.as_str(),
                content,
                source.as_str(),
                created_at.to_rfc3339(),
            ],
        )?;
        Ok(TaskMessage {
            id: conn.last_insert_rowid(),
            task_id: TaskId(task_id.to_string()),
            direction,
            content: content.to_string(),
            source,
            created_at,
            delivered_at: None,
            acked_at: None,
        })
    }

    pub fn list_messages_for_task(&self, task_id: &str) -> Result<Vec<TaskMessage>> {
        let conn = self.db();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM task_messages
             WHERE task_id = ?1 ORDER BY created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(params![task_id], row_to_message)?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn pending_inbound_for_task(&self, task_id: &str) -> Result<Vec<TaskMessage>> {
        let conn = self.db();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM task_messages
             WHERE task_id = ?1 AND direction = 'in' AND delivered_at IS NULL
             ORDER BY created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(params![task_id], row_to_message)?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn mark_delivered(&self, message_id: i64) -> Result<bool> {
        let delivered_at = Local::now().to_rfc3339();
        let rows = self.db().execute(
            "UPDATE task_messages SET delivered_at = ?1
             WHERE id = ?2 AND delivered_at IS NULL",
            params![delivered_at, message_id],
        )?;
        Ok(rows > 0)
    }

    pub fn mark_delivered_matching_inbound(&self, task_id: &str, content: &str) -> Result<bool> {
        let delivered_at = Local::now().to_rfc3339();
        let rows = self.db().execute(
            "UPDATE task_messages SET delivered_at = ?1
             WHERE id = (
                 SELECT id FROM task_messages
                 WHERE task_id = ?2 AND direction = 'in' AND content = ?3 AND delivered_at IS NULL
                 ORDER BY created_at ASC, id ASC
                 LIMIT 1
             )",
            params![delivered_at, task_id, content],
        )?;
        Ok(rows > 0)
    }

    pub fn mark_acked_latest_inbound(&self, task_id: &str) -> Result<bool> {
        let acked_at = Local::now().to_rfc3339();
        let rows = self.db().execute(
            "UPDATE task_messages SET acked_at = ?1
             WHERE id = (
                 SELECT id FROM task_messages
                 WHERE task_id = ?2 AND direction = 'in'
                   AND delivered_at IS NOT NULL AND acked_at IS NULL
                 ORDER BY delivered_at DESC, id DESC
                 LIMIT 1
             )",
            params![acked_at, task_id],
        )?;
        Ok(rows > 0)
    }
}

fn row_to_message(row: &Row<'_>) -> rusqlite::Result<TaskMessage> {
    let direction_value: String = row.get(2)?;
    let source_value: String = row.get(4)?;
    let direction = MessageDirection::try_from(direction_value.as_str())
        .map_err(|_| invalid_text_column(2, direction_value))?;
    let source = MessageSource::try_from(source_value.as_str())
        .map_err(|_| invalid_text_column(4, source_value))?;
    Ok(TaskMessage {
        id: row.get(0)?,
        task_id: TaskId(row.get(1)?),
        direction,
        content: row.get(3)?,
        source,
        created_at: parse_dt(&row.get::<_, String>(5)?),
        delivered_at: row.get::<_, Option<String>>(6)?.map(|value| parse_dt(&value)),
        acked_at: row.get::<_, Option<String>>(7)?.map(|value| parse_dt(&value)),
    })
}

fn invalid_text_column(index: usize, value: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid task message value: {value}"),
        )),
    )
}

#[cfg(test)]
mod tests {
    use chrono::Local;
    use rusqlite::params;

    use super::*;
    use crate::store::Store;

    fn insert_task(store: &Store, task_id: &str) {
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at)
                 VALUES (?1, 'codex', 'prompt', 'running', ?2)",
                params![task_id, Local::now().to_rfc3339()],
            )
            .unwrap();
    }

    #[test]
    fn message_queries_insert_and_list_roundtrip() {
        let store = Store::open_memory().unwrap();
        insert_task(&store, "t-message");

        let inserted = store
            .insert_message("t-message", MessageDirection::In, "hello", MessageSource::Reply)
            .unwrap();
        let messages = store.list_messages_for_task("t-message").unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, inserted.id);
        assert_eq!(messages[0].source, MessageSource::Reply);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn message_queries_pending_filters_delivered_rows() {
        let store = Store::open_memory().unwrap();
        insert_task(&store, "t-pending");
        let pending = store
            .insert_message("t-pending", MessageDirection::In, "pending", MessageSource::Reply)
            .unwrap();
        let delivered = store
            .insert_message("t-pending", MessageDirection::In, "delivered", MessageSource::Reply)
            .unwrap();
        store.mark_delivered(delivered.id).unwrap();

        let messages = store.pending_inbound_for_task("t-pending").unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, pending.id);
    }

    #[test]
    fn message_queries_mark_delivery_and_ack() {
        let store = Store::open_memory().unwrap();
        insert_task(&store, "t-acked");
        let first = store
            .insert_message("t-acked", MessageDirection::In, "first", MessageSource::Reply)
            .unwrap();
        let second = store
            .insert_message("t-acked", MessageDirection::In, "second", MessageSource::Reply)
            .unwrap();
        store.mark_delivered(first.id).unwrap();
        store.mark_delivered(second.id).unwrap();

        assert!(store.mark_acked_latest_inbound("t-acked").unwrap());

        let messages = store.list_messages_for_task("t-acked").unwrap();
        assert!(messages.iter().find(|msg| msg.id == first.id).unwrap().acked_at.is_none());
        assert!(messages.iter().find(|msg| msg.id == second.id).unwrap().acked_at.is_some());
    }

    #[test]
    fn message_queries_match_steer_delivery_by_content() {
        let store = Store::open_memory().unwrap();
        insert_task(&store, "t-steer-delivery");
        let pending = store
            .insert_message(
                "t-steer-delivery",
                MessageDirection::In,
                "follow-up",
                MessageSource::Steer,
            )
            .unwrap();

        assert!(store
            .mark_delivered_matching_inbound("t-steer-delivery", "follow-up")
            .unwrap());

        let messages = store.list_messages_for_task("t-steer-delivery").unwrap();
        assert!(messages
            .iter()
            .find(|msg| msg.id == pending.id)
            .unwrap()
            .delivered_at
            .is_some());
    }
}
