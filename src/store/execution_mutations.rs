// Execution-state Store mutations for background reconciliation.
// Exports active execution failure transitions, the backup_url accessors and
// the backup claim. Deps: rusqlite params and task status guards.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::types::{ACTIVE_EXECUTION_FAILURE_STATUSES, TaskStatus};

impl Store {
    pub fn set_backup_url(&self, id: &str, url: &str) -> Result<bool> {
        let rows = self.db().execute(
            "UPDATE tasks SET backup_url = ?1 WHERE id = ?2",
            params![url, id],
        )?;
        Ok(rows > 0)
    }

    pub fn backup_url(&self, id: &str) -> Result<Option<String>> {
        let url = self
            .db()
            .query_row(
                "SELECT backup_url FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        Ok(url.flatten().filter(|url| !url.is_empty()))
    }

    /// Atomically claims the task's one backup attempt: only the first caller
    /// across all processes sees `true`. The claim is an empty `backup_url`,
    /// which `backup_url` reports as absent until the upload records a URL.
    pub fn claim_backup(&self, id: &str) -> Result<bool> {
        let rows = self.db().execute(
            "UPDATE tasks SET backup_url = '' WHERE id = ?1 AND backup_url IS NULL",
            params![id],
        )?;
        Ok(rows > 0)
    }

    pub fn fail_completed_verify_gate(&self, id: &str) -> Result<bool> {
        let rows = self.db().execute(
            "UPDATE tasks SET status = 'failed',
             exit_code = CASE WHEN exit_code IS NULL OR exit_code = 0 THEN 1 ELSE exit_code END
             WHERE id = ?1 AND status = 'done'",
            params![id],
        )?;
        Ok(rows > 0)
    }

    pub fn fail_active_execution(&self, id: &str) -> Result<bool> {
        if !self.guard_current_status(id, &ACTIVE_EXECUTION_FAILURE_STATUSES, TaskStatus::Failed)? {
            return Ok(false);
        }
        let rows = self.db().execute(
            "UPDATE tasks SET status = 'failed' WHERE id = ?1
             AND status IN (?2, ?3, ?4, ?5)",
            params![
                id,
                ACTIVE_EXECUTION_FAILURE_STATUSES[0].as_str(),
                ACTIVE_EXECUTION_FAILURE_STATUSES[1].as_str(),
                ACTIVE_EXECUTION_FAILURE_STATUSES[2].as_str(),
                ACTIVE_EXECUTION_FAILURE_STATUSES[3].as_str()
            ],
        )?;
        Ok(rows > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn insert_task(store: &Store, id: &str, status: TaskStatus) {
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at)
                 VALUES (?1, 'codex', 'prompt', ?2, '2026-03-15T00:00:00Z')",
                params![id, status.as_str()],
            )
            .expect("insert task");
    }

    #[test]
    fn fail_active_execution_terminalizes_all_failable_execution_states() {
        let store = Store::open_memory().expect("store");
        for status in ACTIVE_EXECUTION_FAILURE_STATUSES {
            let id = format!("t-{}", status.as_str().replace('_', "-"));
            insert_task(&store, &id, status);

            assert!(store.fail_active_execution(&id).expect("fail"));
            assert_eq!(
                store.get_task(&id).expect("get").expect("task").status,
                TaskStatus::Failed
            );
        }
    }

    #[test]
    fn fail_active_execution_does_not_terminalize_other_states() {
        let store = Store::open_memory().expect("store");
        insert_task(&store, "t-pending", TaskStatus::Pending);
        insert_task(&store, "t-done", TaskStatus::Done);

        assert!(!store.fail_active_execution("t-pending").expect("pending"));
        assert!(!store.fail_active_execution("t-done").expect("done"));
        assert_eq!(
            store.get_task("t-pending").expect("get").expect("task").status,
            TaskStatus::Pending
        );
        assert_eq!(
            store.get_task("t-done").expect("get").expect("task").status,
            TaskStatus::Done
        );
    }

    #[test]
    fn fail_completed_verify_gate_marks_done_failed_with_nonzero_exit() {
        let store = Store::open_memory().expect("store");
        insert_task(&store, "t-vfail", TaskStatus::Done);

        assert!(store.fail_completed_verify_gate("t-vfail").expect("fail"));

        let task = store.get_task("t-vfail").expect("get").expect("task");
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.exit_code, Some(1));
    }
}

#[cfg(test)]
mod backup_url_tests {
    use super::*;

    fn insert(store: &Store, id: &str) {
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES (?1, 'codex', 'task', 'done', '2026-09-13T00:00:00Z')",
            params![id],
        ).unwrap();
    }

    #[test]
    fn backup_url_roundtrips_and_is_absent_by_default() {
        let store = Store::open_memory().unwrap();
        insert(&store, "t-backup");
        assert_eq!(store.backup_url("t-backup").unwrap(), None);
        assert!(store.set_backup_url("t-backup", "https://drive.google.com/file/d/x/view").unwrap());
        assert_eq!(
            store.backup_url("t-backup").unwrap().as_deref(),
            Some("https://drive.google.com/file/d/x/view")
        );
        assert!(!store.set_backup_url("t-none", "u").unwrap());
        assert_eq!(store.backup_url("t-none").unwrap(), None);
    }

    #[test]
    fn backup_claim_succeeds_once_and_is_not_a_url() {
        let store = Store::open_memory().unwrap();
        insert(&store, "t-claim");
        assert!(store.claim_backup("t-claim").unwrap());
        assert!(!store.claim_backup("t-claim").unwrap(), "second claim loses");
        assert_eq!(store.backup_url("t-claim").unwrap(), None);
        assert!(!store.claim_backup("t-none").unwrap());
        store.set_backup_url("t-claim", "u").unwrap();
        assert_eq!(store.backup_url("t-claim").unwrap().as_deref(), Some("u"));
    }

    #[test]
    fn migration_adds_backup_url_to_existing_databases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aid.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (id TEXT PRIMARY KEY, agent TEXT NOT NULL, prompt TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', created_at DATETIME NOT NULL);",
        ).unwrap();
        drop(conn);
        let store = Store::open(&path).unwrap();
        insert(&store, "t-old");
        assert!(store.set_backup_url("t-old", "u").unwrap());
        assert_eq!(store.backup_url("t-old").unwrap().as_deref(), Some("u"));
    }
}
