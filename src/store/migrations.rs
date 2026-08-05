// Store schema migrations for feature-specific tables.
// Exports: migrate_task_messages, migrate_declared_task_profile,
//          migrate_observed_model.
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

/// Splits model attribution in two. The pre-existing `model` column keeps what
/// aid *requested*; `observed_model` holds what the CLI reported it actually
/// ran, and stays NULL when the CLI said nothing.
///
/// Historical rows are deliberately not rewritten. They were written by a path
/// that fell back to the requested model whenever the CLI stayed silent
/// (`info.model.as_deref().or(model)`), so each is at best a request and at
/// worst a wrong guess: `t-bd455a68` records the `claude` CLI running
/// `gemini-3.6-flash-low` and `t-702f7bcb` records `agy` running cursor's
/// `composer-2`, both of which failed. Reading them as requests is the only
/// honest interpretation left; back-filling `observed_model` from them would
/// launder guesses into observations.
pub(super) fn migrate_observed_model(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN observed_model TEXT;");
    Ok(())
}

pub(super) fn migrate_declared_task_profile(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_difficulty TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_budget TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_urgency TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_rigor TEXT;");
    Ok(())
}
