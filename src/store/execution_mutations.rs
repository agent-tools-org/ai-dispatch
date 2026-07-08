// Execution-state Store mutations for background reconciliation.
// Exports active execution failure transition.
// Deps: rusqlite params and task status guards.

use anyhow::Result;
use rusqlite::params;

use super::Store;
use crate::types::{ACTIVE_EXECUTION_FAILURE_STATUSES, TaskStatus};

impl Store {
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
}
