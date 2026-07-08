// Worktree-related task query methods.
// Exports active sibling detection for shared worktree cleanup guards.
// Deps: Store, rusqlite, and named task status sets.

use anyhow::Result;
use rusqlite::params;

use super::super::Store;
use crate::types::ACTIVE_TASK_STATUSES;

impl Store {
    /// Check if any active tasks share the same worktree path, excluding a given task.
    pub fn has_active_worktree_siblings(&self, worktree_path: &str, exclude_task_id: &str) -> Result<bool> {
        let conn = self.db();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks \
             WHERE worktree_path = ?1 AND id != ?2 \
             AND status IN (?3, ?4, ?5, ?6, ?7)",
            params![
                worktree_path,
                exclude_task_id,
                ACTIVE_TASK_STATUSES[0].as_str(),
                ACTIVE_TASK_STATUSES[1].as_str(),
                ACTIVE_TASK_STATUSES[2].as_str(),
                ACTIVE_TASK_STATUSES[3].as_str(),
                ACTIVE_TASK_STATUSES[4].as_str()
            ],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}
