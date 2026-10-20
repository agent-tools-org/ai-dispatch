// Private current-attempt permission records, separate from user-facing task events.
// Exports Store persistence helpers and idempotent table creation for existing databases.
// Deps: rusqlite, Store and JSON serialized by the runtime launch boundary.

use super::Store;
use anyhow::Result;
use rusqlite::OptionalExtension;

pub(super) fn create_table(store: &Store) -> Result<()> {
    store.db().execute_batch(
        "CREATE TABLE IF NOT EXISTS task_read_only_launch (
            task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
            recorded_at TEXT NOT NULL,
            record_json TEXT NOT NULL
        );",
    )?;
    Ok(())
}

impl Store {
    pub(crate) fn save_read_only_launch(&self, task: &str, record: &str) -> Result<()> {
        self.db().execute(
            "INSERT INTO task_read_only_launch (task_id, recorded_at, record_json)
             SELECT id, ?2, ?3 FROM tasks WHERE id = ?1 AND read_only = 1
             ON CONFLICT(task_id) DO UPDATE SET
                 recorded_at = excluded.recorded_at, record_json = excluded.record_json",
            rusqlite::params![task, chrono::Local::now().to_rfc3339(), record],
        )?;
        Ok(())
    }

    pub(crate) fn read_only_launch(&self, task: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row(
                "SELECT record_json FROM task_read_only_launch WHERE task_id = ?1",
                [task],
                |row| row.get(0),
            )
            .optional()?)
    }
}
