// Task-related store query methods and task-scoring helpers.
// Exports: Store task lookup, listing, metrics, and similarity methods.
// Deps: super::super::Store, rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::{DateTime, Local};
use rusqlite::{params, OptionalExtension};

use super::super::schema::row_to_task;
use super::super::Store;
use crate::types::{AgentKind, Task, TaskFilter, TaskStatus};

const SIMILAR_TASK_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "that", "this", "have", "your", "task", "code",
    "into", "using", "while", "when", "then", "which",
];

fn extract_similar_keywords(prompt: &str) -> Vec<String> {
    let mut candidates: Vec<(String, usize)> = prompt
        .split_whitespace()
        .filter_map(|word| {
            let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());
            if cleaned.len() < 4 {
                return None;
            }
            let lower = cleaned.to_lowercase();
            if SIMILAR_TASK_STOPWORDS.contains(&lower.as_str()) {
                return None;
            }
            Some((lower, cleaned.len()))
        })
        .collect();
    candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    candidates.truncate(3);
    candidates.into_iter().map(|(word, _)| word).collect()
}

impl Store {
    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
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
                 completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
            TaskFilter::Running => (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                 log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
                 completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
                 FROM tasks WHERE status IN (?1, ?2) ORDER BY created_at DESC",
                vec!["running".to_string(), "awaiting_input".to_string()],
            ),
            TaskFilter::Today => {
                let today = Local::now().format("%Y-%m-%d").to_string();
                (
                    "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                     caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                     log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
                     completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
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
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
             FROM tasks
             WHERE agent = ?1 AND status = ?2 AND duration_ms IS NOT NULL AND created_at >= ?3
             ORDER BY created_at DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![agent.as_str(), TaskStatus::Done.as_str(), cutoff, limit], row_to_task)?;
        rows.map(|row| row?).collect()
    }

    pub fn budget_usage_summary(
        &self,
        agent: &str,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        let conn = self.db();
        let (task_count, total_tokens, total_cost): (i64, i64, f64) = conn.query_row(
            "SELECT COUNT(*) as task_count,
                    COALESCE(SUM(tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost
             FROM tasks WHERE agent = ?1 AND (?2 IS NULL OR created_at >= ?2)",
            params![agent, since.map(|value| value.to_rfc3339())],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((u32::try_from(task_count)?, total_tokens, total_cost))
    }

    pub fn list_tasks_by_session(&self, session_id: &str) -> Result<Vec<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
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
             completed_at, verify, read_only, budget, custom_agent_name, verify_status, exit_code, category, pending_reason
             FROM tasks WHERE workgroup_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![group_id], row_to_task)?;
        rows.map(|row| row?).collect()
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

    pub fn agent_success_rates_by_category(&self, category: &str) -> Result<Vec<(AgentKind, f64, usize)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent,
                    SUM(CASE WHEN status IN ('done', 'merged') THEN 1 ELSE 0 END) as successes,
                    COUNT(*) as total
             FROM tasks
             WHERE status IN ('done', 'merged', 'failed') AND category = ?1
             GROUP BY agent
             HAVING total >= 5",
        )?;
        let rows = stmt.query_map(params![category], |row| {
            let agent_str: String = row.get(0)?;
            let successes: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let rate = successes as f64 / total as f64;
            Ok((agent, rate, total as usize))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn agent_avg_costs(&self) -> Result<Vec<(AgentKind, f64)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, AVG(cost_usd) as avg_cost
             FROM tasks
             WHERE cost_usd IS NOT NULL AND cost_usd > 0
             GROUP BY agent
             HAVING COUNT(*) >= 3",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let avg_cost: f64 = row.get(1)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            Ok((agent, avg_cost))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn find_similar_tasks(
        &self,
        prompt: &str,
        limit: usize,
    ) -> Result<Vec<(String, AgentKind, TaskStatus)>> {
        let keywords = extract_similar_keywords(prompt);
        if limit == 0 || keywords.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, status, prompt FROM tasks
             WHERE status IN ('done', 'failed', 'merged')
             ORDER BY created_at DESC
             LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let agent_str: String = row.get(1)?;
            let status_str: String = row.get(2)?;
            let task_prompt: String = row.get(3)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let status = TaskStatus::parse_str(&status_str).unwrap_or(TaskStatus::Failed);
            Ok((id, agent, status, task_prompt))
        })?;
        let mut scored = Vec::new();
        for row in rows {
            let (id, agent, status, task_prompt) = row?;
            let lower_prompt = task_prompt.to_lowercase();
            let score: usize = keywords
                .iter()
                .map(|keyword| lower_prompt.matches(keyword).count())
                .sum();
            if score > 0 {
                scored.push((score, id, agent, status));
            }
        }
        scored.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        scored.truncate(limit);
        Ok(scored
            .into_iter()
            .map(|(_, id, agent, status)| (id, agent, status))
            .collect())
    }

    /// Check if any non-terminal tasks share the same worktree path, excluding a given task.
    pub fn has_active_worktree_siblings(&self, worktree_path: &str, exclude_task_id: &str) -> Result<bool> {
        let conn = self.db();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks \
             WHERE worktree_path = ?1 AND id != ?2 \
             AND status IN ('pending', 'running', 'waiting', 'awaiting_input')",
            params![worktree_path, exclude_task_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}
