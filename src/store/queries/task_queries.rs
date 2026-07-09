// Task-related store query methods and task-scoring helpers.
// Exports: Store task lookup, listing, metrics, and similarity methods.
// Deps: super::super::Store, rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::Local;
use rusqlite::{params, OptionalExtension};

use super::super::schema::row_to_task;
use super::super::Store;
use crate::types::{AgentKind, Task, TaskFilter, TaskStatus, ACTIVE_TASK_STATUSES};

impl Store {
    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
             FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_task)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn get_completion_summary(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.db();
        let summary = conn
            .query_row(
                "SELECT completion_summary FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        Ok(summary.flatten().filter(|value| !value.is_empty()))
    }

    pub fn get_task_dispatch_args(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.db();
        conn.query_row(
            "SELECT dispatch_args FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(Into::into)
    }

    pub fn get_retry_chain(&self, task_id: &str) -> Result<Vec<Task>> {
        let mut chain = Vec::new();
        let mut current = self.get_task(task_id)?;
        while let Some(task) = current {
            let parent_task_id = task.parent_task_id.clone();
            chain.push(task);
            current = match parent_task_id {
                Some(parent_id) => self.get_task(&parent_id)?,
                None => None,
            };
        }
        chain.reverse();
        Ok(chain)
    }

    /// Returns the root of the retry chain that contains `task_id`.
    /// If `task_id` has no parent, it IS the root.
    pub fn find_retry_root(&self, task_id: &str) -> Result<Option<Task>> {
        let chain = self.get_retry_chain(task_id)?;
        Ok(chain.into_iter().next())
    }

    /// Returns `root_task` plus every transitive retry descendant (BFS),
    /// ordered with the root first. Used by `aid stop --retry-tree`.
    pub fn get_retry_tree(&self, root_id: &str) -> Result<Vec<Task>> {
        let root = match self.get_task(root_id)? {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };
        let mut out = vec![root];
        let mut frontier = vec![root_id.to_string()];
        while let Some(parent_id) = frontier.pop() {
            let children = self.get_direct_retries(&parent_id)?;
            for child in children {
                frontier.push(child.id.0.clone());
                out.push(child);
            }
        }
        Ok(out)
    }

    /// Direct retry children: tasks where `parent_task_id == parent_id`.
    pub fn get_direct_retries(&self, parent_id: &str) -> Result<Vec<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
             FROM tasks WHERE parent_task_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![parent_id], row_to_task)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    pub fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        let conn = self.db();
        let (sql, filter_params): (&str, Vec<String>) = match filter {
            TaskFilter::All => (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                 created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                 exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
            TaskFilter::Active => (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                 created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                 exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
                 FROM tasks WHERE status IN (?1, ?2, ?3, ?4, ?5) ORDER BY created_at DESC",
                ACTIVE_TASK_STATUSES
                    .iter()
                    .map(|status| status.as_str().to_string())
                    .collect(),
            ),
            TaskFilter::Running => (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                 created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                 exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
                 FROM tasks WHERE status IN (?1, ?2, ?3) ORDER BY created_at DESC",
                vec![
                    "running".to_string(),
                    "awaiting_input".to_string(),
                    "stalled".to_string(),
                ],
            ),
            TaskFilter::Today => {
                let today = Local::now().format("%Y-%m-%d").to_string();
                (
                    "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                     caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                     start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                     created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                     exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
                     FROM tasks WHERE created_at >= ?1 ORDER BY created_at DESC",
                    vec![today],
                )
            }
        };
        let mut stmt = conn.prepare(sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            filter_params.iter().map(|value| value as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), row_to_task)?;
        rows.map(|row| row?).collect()
    }

    /// Latest model string from the most recently created successful builtin task for `agent`.
    pub fn latest_default_model(&self, agent: AgentKind) -> Result<Option<String>> {
        let tasks = self.list_tasks(TaskFilter::All)?;
        for task in tasks {
            if task.agent != agent {
                continue;
            }
            if !matches!(task.status, TaskStatus::Done | TaskStatus::Merged) {
                continue;
            }
            if let Some(raw) = task.model.as_deref() {
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    return Ok(Some(trimmed.to_string()));
                }
            }
        }
        Ok(None)
    }

    pub fn list_running_tasks(&self) -> Result<Vec<Task>> {
        self.list_tasks(TaskFilter::Running)
    }

    pub fn recent_tasks_for_agent(&self, agent: AgentKind, limit: usize) -> Result<Vec<Task>> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let conn = self.db();
        let cutoff = (Local::now() - chrono::Duration::days(7)).to_rfc3339();
        let limit = i64::try_from(limit)?;
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
             FROM tasks
             WHERE agent = ?1 AND status = ?2 AND duration_ms IS NOT NULL AND created_at >= ?3
             ORDER BY created_at DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![agent.as_str(), TaskStatus::Done.as_str(), cutoff, limit], row_to_task)?;
        rows.map(|row| row?).collect()
    }

    pub fn list_tasks_by_session(&self, session_id: &str) -> Result<Vec<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
             FROM tasks WHERE caller_session_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], row_to_task)?;
        rows.map(|row| row?).collect()
    }

    pub fn list_tasks_by_group(&self, group_id: &str) -> Result<Vec<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, final_head_sha, final_branch
             FROM tasks WHERE workgroup_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![group_id], row_to_task)?;
        rows.map(|row| row?).collect()
    }

}
