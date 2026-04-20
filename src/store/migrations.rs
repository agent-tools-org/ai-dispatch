// Store schema migrations for feature-specific tables.
// Exports: migrate_task_messages.
// Deps: anyhow and rusqlite.

use anyhow::Result;
use rusqlite::Connection;

const CREATE_TASK_MESSAGES_SQL: &str = "CREATE TABLE IF NOT EXISTS task_messages (
    id INTEGER PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    direction TEXT NOT NULL CHECK (direction IN ('in','out')),
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('reply','steer','unstick-auto','agent-ack')),
    created_at DATETIME NOT NULL,
    delivered_at DATETIME,
    acked_at DATETIME
);";

pub(super) fn migrate_task_messages(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_TASK_MESSAGES_SQL)?;
    Ok(())
}
