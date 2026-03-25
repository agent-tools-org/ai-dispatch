// Extra workgroup persistence for lifecycle commands.
// Extends Store with update/delete flows while keeping store.rs smaller.
// Depends on rusqlite, chrono, and the core Store type.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use crate::store::Store;

impl Store {
    pub fn update_workgroup(
        &self,
        id: &str,
        name: Option<&str>,
        shared_context: Option<&str>,
    ) -> Result<Option<crate::types::Workgroup>> {
        let Some(mut workgroup) = self.get_workgroup(id)? else {
            return Ok(None);
        };
        if let Some(name) = name {
            workgroup.name = name.to_string();
        }
        if let Some(shared_context) = shared_context {
            workgroup.shared_context = shared_context.to_string();
        }
        workgroup.updated_at = Local::now();

        self.db().execute(
            "UPDATE workgroups SET name = ?1, shared_context = ?2, updated_at = ?3
             WHERE id = ?4",
            params![
                workgroup.name,
                workgroup.shared_context,
                workgroup.updated_at.to_rfc3339(),
                workgroup.id.as_str(),
            ],
        )?;

        Ok(Some(workgroup))
    }

    pub fn delete_workgroup(&self, id: &str) -> Result<Option<usize>> {
        let tagged_tasks = self.count_workgroup_tasks(id)?;
        let deleted = self
            .db()
            .execute("DELETE FROM workgroups WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Ok(None);
        }
        let workspace_dir = crate::paths::workspace_dir(id)?;
        let ws = workspace_dir.to_string_lossy();
        if ws.starts_with("/tmp/aid-wg-") || ws.starts_with("/private/tmp/aid-wg-") {
            let _ = std::fs::remove_dir_all(&workspace_dir);
        } else {
            aid_warn!(
                "[aid] SAFETY: refusing to remove workspace '{}' — not under /tmp/aid-wg-*",
                ws
            );
        }
        Ok(Some(tagged_tasks))
    }

    fn count_workgroup_tasks(&self, id: &str) -> Result<usize> {
        let count = self.db().query_row(
            "SELECT COUNT(*) FROM tasks WHERE workgroup_id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn make_task(id: &str, group_id: &str) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: Some(group_id.to_string()),
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn update_workgroup_changes_requested_fields() {
        let store = Store::open_memory().unwrap();
        let workgroup = store
            .create_workgroup("dispatch", "Shared repo rules.", None, None)
            .unwrap();

        let updated = store
            .update_workgroup(
                workgroup.id.as_str(),
                Some("dispatch-core"),
                Some("Updated shared rules."),
            )
            .unwrap()
            .unwrap();

        assert_eq!(updated.name, "dispatch-core");
        assert_eq!(updated.shared_context, "Updated shared rules.");
        assert!(updated.updated_at >= updated.created_at);
    }

    #[test]
    fn delete_workgroup_keeps_historical_task_tags() {
        let store = Store::open_memory().unwrap();
        let workgroup = store
            .create_workgroup("dispatch", "Shared repo rules.", None, None)
            .unwrap();
        store
            .insert_task(&make_task("t-1000", workgroup.id.as_str()))
            .unwrap();

        let tagged_tasks = store.delete_workgroup(workgroup.id.as_str()).unwrap();
        let task = store.get_task("t-1000").unwrap().unwrap();

        assert_eq!(tagged_tasks, Some(1));
        assert_eq!(task.workgroup_id.as_deref(), Some(workgroup.id.as_str()));
        assert!(store
            .get_workgroup(workgroup.id.as_str())
            .unwrap()
            .is_none());
    }
}
