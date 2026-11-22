// Durable record of settled tasks owed to the session that dispatched them.
// A row stays pending until that session reads the task; a push claim is separate.
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use super::Store;

pub(super) fn create_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS caller_inbox (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             caller_session_id TEXT NOT NULL,
             recorded_at TEXT NOT NULL,
             pushed_at TEXT,
             seen_at TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_caller_inbox_pending
             ON caller_inbox(caller_session_id) WHERE seen_at IS NULL;",
    )?;
    Ok(())
}

pub(super) fn has_table(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'caller_inbox')",
        [],
        |row| row.get(0),
    )?)
}

/// A settled task the caller has not read yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingCompletion {
    pub task_id: String,
    pub status: String,
}

impl Store {
    /// Records a settled task for its caller session. Idempotent.
    pub(crate) fn inbox_record(&self, task_id: &str, session: &str) -> Result<()> {
        self.db().execute(
            "INSERT OR IGNORE INTO caller_inbox (task_id, caller_session_id, recorded_at)
             VALUES (?1, ?2, ?3)",
            params![task_id, session, chrono::Local::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Claims the single push for a task; false when another process already claimed it.
    pub(crate) fn inbox_claim_push(&self, task_id: &str) -> Result<bool> {
        let rows = self.db().execute(
            "UPDATE caller_inbox SET pushed_at = ?2 WHERE task_id = ?1 AND pushed_at IS NULL",
            params![task_id, chrono::Local::now().to_rfc3339()],
        )?;
        Ok(rows == 1)
    }

    /// Marks a task read by its caller session; other sessions do not acknowledge it.
    pub(crate) fn inbox_mark_seen(&self, task_id: &str, session: &str) -> Result<()> {
        self.db().execute(
            "UPDATE caller_inbox SET seen_at = ?3
             WHERE task_id = ?1 AND caller_session_id = ?2 AND seen_at IS NULL",
            params![task_id, session, chrono::Local::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Settled tasks the session has not read, oldest first.
    pub(crate) fn inbox_pending(&self, session: &str, limit: usize) -> Result<Vec<PendingCompletion>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT i.task_id, t.status FROM caller_inbox i JOIN tasks t ON t.id = i.task_id
             WHERE i.caller_session_id = ?1 AND i.seen_at IS NULL
             ORDER BY i.recorded_at LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![session, limit as i64], |row| {
            Ok(PendingCompletion { task_id: row.get(0)?, status: row.get(1)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    #[cfg(test)]
    pub(crate) fn inbox_pushed_at(&self, task_id: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row(
                "SELECT pushed_at FROM caller_inbox WHERE task_id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }
}

#[cfg(test)]
#[path = "caller_inbox_tests.rs"]
mod tests;
