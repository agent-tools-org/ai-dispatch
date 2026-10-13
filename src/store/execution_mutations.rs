// Execution-state Store mutations for background reconciliation.
// Exports active execution failure transitions, the backup_url accessors and
// the backup claim. Deps: rusqlite params and task status guards.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::types::{ACTIVE_EXECUTION_FAILURE_STATUSES, TaskStatus, Task, AgentKind};

impl Store {
    /// Replace a waiting task's row with full task data (called when dispatch begins).
    /// The only legal move is the waiting row -> the dispatch status; anything else
    /// is rejected by the status guard and aborts the dispatch.
    pub fn replace_waiting_task(&self, task: &Task) -> Result<()> {
        let id = task.id.as_str();
        if !self.guard_current_status(id, &[TaskStatus::Waiting], task.status)? {
            anyhow::bail!(
                "Task '{id}' is not waiting; refusing to replace its row with status '{}'",
                task.status.as_str()
            );
        }
        let agent_value = if task.agent == AgentKind::Custom {
            task.custom_agent_name.as_deref().unwrap_or("custom")
        } else {
            task.agent.as_str()
        };
        let rows = self.db().execute(
            "UPDATE tasks SET agent=?2, prompt=?3, resolved_prompt=?4, status=?5,
             parent_task_id=?6, workgroup_id=?7, caller_kind=?8, caller_session_id=?9,
             agent_session_id=?10, repo_path=?11, project_id=?12, worktree_path=?13, worktree_branch=?14,
             final_head_sha=?15, final_branch=?16, start_sha=?17, log_path=?18,
             output_path=?19, model=?20, verify=?21,
             verify_status=?22, read_only=?23, budget=?24, custom_agent_name=?25,
             category=?26, pending_reason=?27, audit_verdict=?28, audit_report_path=?29,
             delivery_assessment=?30, effective_dir=?31
             WHERE id=?1 AND status='waiting'",
            params![
                task.id.as_str(), agent_value, task.prompt, task.resolved_prompt,
                task.status.as_str(), task.parent_task_id, task.workgroup_id,
                task.caller_kind, task.caller_session_id, task.agent_session_id,
                task.repo_path, task.project_id, task.worktree_path, task.worktree_branch,
                task.final_head_sha, task.final_branch,
                task.start_sha, task.log_path, task.output_path, task.requested_model,
                task.verify, task.verify_status.as_str(), task.read_only,
                task.budget, task.custom_agent_name, task.category,
                task.pending_reason,
                task.audit_verdict,
                task.audit_report_path,
                task.delivery_assessment.map(|value| value.as_str()),
                task.effective_dir,
            ],
        )?;
        if rows == 0 {
            anyhow::bail!("Task '{id}' left waiting state; refusing dispatch");
        }
        Ok(())
    }

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

    #[test]
    fn migration_normalizes_historical_backup_attempts_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aid.db");
        let store = Store::open(&path).unwrap();
        for id in ["t-failed", "t-uploaded", "t-null", "t-invalid", "t-fresh", "t-claimed"] {
            insert(&store, id);
        }
        store.set_backup_url("t-uploaded", "existing-url").unwrap();
        assert!(store.claim_backup("t-claimed").unwrap());
        for (id, metadata) in [
            ("t-failed", "{\"backup\":\"failed\"}"),
            ("t-uploaded", "{\"backup\":\"uploaded\"}"),
            ("t-null", "{\"backup\":null}"),
            ("t-invalid", "invalid json"),
            ("t-fresh", "{\"other\":\"failed\"}"),
        ] {
            store.db().execute(
                "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
                 VALUES (?1, '2026-09-13T00:00:00Z', 'milestone', 'historical event', ?2)",
                params![id, metadata],
            ).unwrap();
        }
        store.db().execute_batch("PRAGMA user_version = 0;").unwrap();
        drop(store);

        let store = Store::open(&path).unwrap();
        for id in ["t-failed", "t-uploaded", "t-null", "t-claimed"] {
            assert!(!store.claim_backup(id).unwrap(), "{id}");
        }
        assert_eq!(store.backup_url("t-failed").unwrap(), None);
        assert_eq!(store.backup_url("t-uploaded").unwrap().as_deref(), Some("existing-url"));
        assert!(store.claim_backup("t-invalid").unwrap());
        assert!(store.claim_backup("t-fresh").unwrap());
        assert_eq!(store.get_events("t-failed").unwrap().len(), 1);
        insert(&store, "t-report-only");
        store.db().execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES ('t-report-only', '2026-09-13T00:00:00Z', 'milestone', 'report', '{\"backup\":\"failed\"}')",
            [],
        ).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(store.claim_backup("t-report-only").unwrap(), "events are reporting only after migration");
    }
}
