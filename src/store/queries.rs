// Store read operations for tasks, workgroups, and events.
// Exports: Store query methods.
// Deps: rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::Local;
use rusqlite::{params, OptionalExtension};

use super::schema::{parse_dt, row_to_event, row_to_memory, row_to_task};
use super::Store;
use crate::types::*;

impl Store {
    pub fn get_workgroup(&self, id: &str) -> Result<Option<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at
             FROM workgroups WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_workgroup)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn list_workgroups(&self) -> Result<Vec<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at
             FROM workgroups ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_workgroup)?;
        rows.map(|row| row?).collect()
    }

    pub fn latest_milestone(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.db();
        let milestone = conn
            .query_row(
                "SELECT detail FROM events
                 WHERE task_id = ?1 AND event_type = 'milestone'
                 ORDER BY timestamp DESC
                 LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(milestone)
    }

    pub fn get_workgroup_milestones(&self, workgroup_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT e.task_id, e.detail FROM events e
             JOIN tasks t ON e.task_id = t.id
             WHERE t.workgroup_id = ?1 AND e.event_type = 'milestone'
             ORDER BY e.timestamp ASC",
        )?;
        let rows = stmt.query_map(params![workgroup_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn list_findings(&self, workgroup_id: &str) -> Result<Vec<Finding>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, workgroup_id, content, source_task_id, created_at FROM findings
             WHERE workgroup_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![workgroup_id], |row| {
            Ok(Finding {
                id: row.get(0)?,
                workgroup_id: row.get(1)?,
                content: row.get(2)?,
                source_task_id: row.get(3)?,
                created_at: parse_dt(&row.get::<_, String>(4)?),
            })
        })?;
        rows.map(|r| Ok(r?)).collect()
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, read_only, budget, custom_agent_name, verify_status
             FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_task)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
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

    pub fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        let conn = self.db();
        let (sql, filter_params): (&str, Vec<String>) = match filter {
            TaskFilter::All => (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
                 completed_at, verify, read_only, budget, custom_agent_name, verify_status
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
            TaskFilter::Running => (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
                 completed_at, verify, read_only, budget, custom_agent_name, verify_status
                 FROM tasks WHERE status IN (?1, ?2) ORDER BY created_at DESC",
                vec!["running".to_string(), "awaiting_input".to_string()],
            ),
            TaskFilter::Today => (
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
                 completed_at, verify, read_only, budget, custom_agent_name, verify_status
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = filter_params
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), row_to_task)?;
        let mut tasks = rows.map(|r| r?).collect::<Result<Vec<_>>>()?;
        if matches!(filter, TaskFilter::Today) {
            let today = Local::now().date_naive();
            tasks.retain(|task| task.created_at.date_naive() == today);
        }
        Ok(tasks)
    }

    pub fn list_running_tasks(&self) -> Result<Vec<Task>> {
        self.list_tasks(TaskFilter::Running)
    }

    pub fn list_tasks_by_session(&self, session_id: &str) -> Result<Vec<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, read_only, budget, custom_agent_name, verify_status
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
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, read_only, budget, custom_agent_name, verify_status
             FROM tasks WHERE workgroup_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![group_id], row_to_task)?;
        rows.map(|row| row?).collect()
    }

    pub fn get_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT task_id, timestamp, event_type, detail, metadata
             FROM events WHERE task_id = ?1 ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![task_id], row_to_event)?;
        rows.map(|r| Ok(r?)).collect()
    }

    pub fn agent_success_rates(&self) -> Result<Vec<(AgentKind, f64, usize)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, 
                    SUM(CASE WHEN status IN ('done', 'merged') THEN 1 ELSE 0 END) as successes,
                    COUNT(*) as total
             FROM tasks 
             WHERE status IN ('done', 'merged', 'failed')
             GROUP BY agent
             HAVING total >= 5",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let successes: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let rate = successes as f64 / total as f64;
            Ok((agent, rate, total as usize))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn list_memories(
        &self,
        project_path: Option<&str>,
        memory_type: Option<MemoryType>,
    ) -> Result<Vec<Memory>> {
        let conn = self.db();
        let now = Local::now().to_rfc3339();
        let type_value = memory_type.map(|mt| mt.as_str().to_string());
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories
             WHERE (?1 IS NULL OR project_path = ?1)
               AND (?2 IS NULL OR memory_type = ?2)
               AND (expires_at IS NULL OR expires_at > ?3)
               AND id NOT IN (
                   SELECT DISTINCT supersedes FROM memories WHERE supersedes IS NOT NULL
               )
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(
            params![project_path, type_value.as_deref(), now],
            row_to_memory,
        )?;
        let memories = rows.map(|row| row?).collect::<Result<Vec<_>>>()?;
        Ok(memories)
    }

    pub fn memory_history(&self, id: &str) -> Result<Vec<Memory>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories WHERE id = ?1",
        )?;
        let mut child_stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories WHERE supersedes = ?1
             ORDER BY version ASC
             LIMIT 1",
        )?;
        let base_memory = match stmt.query_row(params![id], |row| row_to_memory(row)) {
            Ok(row) => row?,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(vec![]),
            Err(err) => return Err(err.into()),
        };
        let mut history = Vec::new();
        history.push(base_memory.clone());

        let mut previous_id = base_memory
            .supersedes
            .as_ref()
            .map(|sup| sup.as_str().to_string());
        for _ in 0..50 {
            let prev = match previous_id {
                Some(ref prev) => prev.clone(),
                None => break,
            };
            match stmt.query_row(params![prev], |row| row_to_memory(row)) {
                Ok(row) => {
                    let memory = row?;
                    previous_id = memory.supersedes.as_ref().map(|sup| sup.as_str().to_string());
                    history.push(memory);
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => break,
                Err(err) => return Err(err.into()),
            }
        }

        let mut next_id = Some(base_memory.id.as_str().to_string());
        for _ in 0..50 {
            let curr = match next_id {
                Some(ref value) => value.clone(),
                None => break,
            };
            match child_stmt.query_row(params![curr], |row| row_to_memory(row)) {
                Ok(row) => {
                    let memory = row?;
                    next_id = Some(memory.id.as_str().to_string());
                    history.push(memory);
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => break,
                Err(err) => return Err(err.into()),
            }
        }

        history.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(history)
    }

    pub fn search_memories(
        &self,
        query: &str,
        project_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let conn = self.db();
        let now = Local::now().to_rfc3339();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories
             WHERE content LIKE ?1
               AND (?2 IS NULL OR project_path = ?2)
               AND (expires_at IS NULL OR expires_at > ?3)
             ORDER BY created_at DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![pattern, project_path, now, limit as i64],
            row_to_memory,
        )?;
        let memories = rows.map(|row| row?).collect::<Result<Vec<_>>>()?;
        Ok(memories)
    }

}

fn row_to_workgroup(row: &rusqlite::Row) -> rusqlite::Result<Result<Workgroup>> {
    Ok(Ok(Workgroup {
        id: WorkgroupId(row.get::<_, String>(0)?),
        name: row.get(1)?,
        shared_context: row.get(2)?,
        created_at: parse_dt(&row.get::<_, String>(3)?),
        updated_at: parse_dt(&row.get::<_, String>(4)?),
    }))
}
