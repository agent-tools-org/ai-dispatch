// Store read operations for tasks, workgroups, and events.
// Exports: Store query methods.
// Deps: rusqlite, chrono, crate::types.

use std::collections::HashMap;

use anyhow::Result;
use chrono::Local;
use rusqlite::{params, OptionalExtension};

use super::schema::{parse_dt, row_to_event, row_to_memory, row_to_task};
use super::Store;
use crate::types::*;

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

    /// Batch fetch latest milestone for multiple tasks in a single query.
    pub fn latest_milestones_batch(&self, task_ids: &[&str]) -> Result<HashMap<String, String>> {
        if task_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.db();
        let placeholders: Vec<String> = (1..=task_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT task_id, detail FROM events e1
             WHERE event_type = 'milestone'
             AND timestamp = (
                 SELECT MAX(timestamp) FROM events e2
                 WHERE e2.task_id = e1.task_id AND e2.event_type = 'milestone'
             )
             AND task_id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = task_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (tid, detail) = row?;
            map.insert(tid, detail);
        }
        Ok(map)
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

    pub fn get_completion_summary(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.db();
        let summary = conn
            .query_row(
                "SELECT completion_summary FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        Ok(summary.flatten().filter(|s| !s.is_empty()))
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
            TaskFilter::Today => {
                let today = Local::now().format("%Y-%m-%d").to_string();
                (
                "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                     caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
                     log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
                     completed_at, verify, read_only, budget, custom_agent_name, verify_status
                     FROM tasks WHERE created_at >= ?1 ORDER BY created_at DESC",
                    vec![today],
                )
            },
        };
        let mut stmt = conn.prepare(sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = filter_params
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), row_to_task)?;
        let tasks = rows.map(|r| r?).collect::<Result<Vec<_>>>()?;
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
            let prompt_text: String = row.get(3)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let status = TaskStatus::parse_str(&status_str).unwrap_or(TaskStatus::Failed);
            Ok((id, agent, status, prompt_text))
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
        let base_memory = match stmt.query_row(params![id], row_to_memory) {
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
            match stmt.query_row(params![prev], row_to_memory) {
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
            match child_stmt.query_row(params![curr], row_to_memory) {
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

#[cfg(test)]
mod tests {
    use crate::store::Store;
    use crate::types::{AgentKind, TaskStatus};
    use rusqlite::params;

    fn insert_task(
        store: &Store,
        id: &str,
        agent: AgentKind,
        status: TaskStatus,
        prompt: &str,
    ) {
        let conn = store.db();
        conn.execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, agent.as_str(), prompt, status.as_str(), "2026-03-15T00:00:00Z"],
        )
        .unwrap();
    }

    #[test]
    fn finds_matching_tasks_with_keyword_scores() {
        let store = Store::open_memory().unwrap();
        insert_task(
            &store,
            "t-routing",
            AgentKind::Codex,
            TaskStatus::Done,
            "Implement cross-session hints for agent selection",
        );
        insert_task(
            &store,
            "t-gemini",
            AgentKind::Gemini,
            TaskStatus::Done,
            "Document routing hints and selection strategy",
        );
        insert_task(
            &store,
            "t-cursor",
            AgentKind::Cursor,
            TaskStatus::Failed,
            "Cleanup the build pipeline wiring",
        );

        let results = store
            .find_similar_tasks("Add routing hints for agent selection", 5)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "t-gemini");
        assert_eq!(results[0].1, AgentKind::Gemini);
        assert_eq!(results[1].0, "t-routing");
        assert_eq!(results[1].1, AgentKind::Codex);
    }

    #[test]
    fn includes_merged_tasks_in_results() {
        let store = Store::open_memory().unwrap();
        insert_task(
            &store,
            "t-merged",
            AgentKind::Codex,
            TaskStatus::Merged,
            "Implement routing hints for budget selection",
        );
        let results = store
            .find_similar_tasks("Add routing hints for selection", 5)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "t-merged");
        assert_eq!(results[0].2, TaskStatus::Merged);
    }

    #[test]
    fn limits_results_to_positive_scores() {
        let store = Store::open_memory().unwrap();
        insert_task(
            &store,
            "t-empty",
            AgentKind::Kilo,
            TaskStatus::Done,
            "Refactor the logging toolchain",
        );
        let results = store.find_similar_tasks("Short", 3).unwrap();
        assert!(results.is_empty());
    }
}
