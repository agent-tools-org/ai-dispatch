// Store write operations for tasks, workgroups, and events.
// Exports: Store mutation methods.
// Deps: rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use super::schema::row_to_memory;
use super::Store;
use crate::types::*;

pub struct TaskCompletionUpdate<'a> {
    pub id: &'a str,
    pub status: TaskStatus,
    pub tokens: Option<i64>,
    pub duration_ms: i64,
    pub model: Option<&'a str>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
}

impl Store {
    pub fn insert_task(&self, task: &Task) -> Result<()> {
        let agent_value = if task.agent == AgentKind::Custom {
            task.custom_agent_name.as_deref().unwrap_or("custom")
        } else {
            task.agent.as_str()
        };
        self.db().execute(
            "INSERT INTO tasks (id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, created_at,
             completed_at, verify, verify_status, read_only, budget, custom_agent_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
             ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
            params![
                task.id.as_str(),
                agent_value,
                task.prompt,
                task.resolved_prompt,
                task.status.as_str(),
                task.parent_task_id,
                task.workgroup_id,
                task.caller_kind,
                task.caller_session_id,
                task.agent_session_id,
                task.repo_path,
                task.worktree_path,
                task.worktree_branch,
                task.log_path,
                task.output_path,
                task.tokens,
                task.prompt_tokens,
                task.duration_ms,
                task.model,
                task.cost_usd,
                task.created_at.to_rfc3339(),
                task.completed_at.map(|t| t.to_rfc3339()),
                task.verify,
                task.verify_status.as_str(),
                task.read_only,
                task.budget,
                task.custom_agent_name,
            ],
        )?;
        Ok(())
    }

    pub fn create_workgroup(&self, name: &str, shared_context: &str) -> Result<Workgroup> {
        let now = Local::now();
        let workgroup = Workgroup {
            id: WorkgroupId::generate(),
            name: name.to_string(),
            shared_context: shared_context.to_string(),
            created_at: now,
            updated_at: now,
        };
        self.db().execute(
            "INSERT INTO workgroups (id, name, shared_context, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                workgroup.id.as_str(),
                workgroup.name,
                workgroup.shared_context,
                workgroup.created_at.to_rfc3339(),
                workgroup.updated_at.to_rfc3339(),
            ],
        )?;
        let workspace_dir = crate::paths::workspace_dir(workgroup.id.as_str());
        let _ = std::fs::create_dir_all(&workspace_dir);
        Ok(workgroup)
    }

    pub fn update_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn update_resolved_prompt(&self, id: &str, resolved_prompt: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET resolved_prompt = ?1 WHERE id = ?2",
            params![resolved_prompt, id],
        )?;
        Ok(())
    }

    pub fn update_prompt_tokens(&self, id: &str, tokens: i64) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET prompt_tokens = ?1 WHERE id = ?2",
            params![tokens, id],
        )?;
        Ok(())
    }

    pub fn update_agent_session_id(&self, id: &str, session_id: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET agent_session_id = ?1 WHERE id = ?2",
            params![session_id, id],
        )?;
        Ok(())
    }

    pub fn update_task_completion(
        &self,
        payload: TaskCompletionUpdate<'_>,
    ) -> Result<()> {
        let now = Local::now().to_rfc3339();
        self.db().execute(
            "UPDATE tasks SET status = ?1, tokens = ?2, duration_ms = ?3, completed_at = ?4,
             model = ?5, cost_usd = ?6, exit_code = ?7 WHERE id = ?8",
            params![
                payload.status.as_str(),
                payload.tokens,
                payload.duration_ms,
                now,
                payload.model,
                payload.cost_usd,
                payload.exit_code,
                payload.id
            ],
        )?;
        Ok(())
    }

    pub fn save_completion_summary(&self, task_id: &str, summary_json: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET completion_summary = ?1 WHERE id = ?2",
            params![summary_json, task_id],
        )?;
        Ok(())
    }

    pub fn save_peer_review(&self, task_id: &str, reviewer: &str, score: u8, feedback: &str) -> Result<()> {
        let review_json = serde_json::json!({
            "reviewer": reviewer,
            "score": score,
            "feedback": feedback,
        })
        .to_string();
        self.db().execute(
            "UPDATE tasks SET peer_review = ?1 WHERE id = ?2",
            params![review_json, task_id],
        )?;
        Ok(())
    }

    pub fn insert_event(&self, event: &TaskEvent) -> Result<()> {
        let metadata_str = event.metadata.as_ref().map(|m| m.to_string());
        self.db().execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.task_id.as_str(),
                event.timestamp.to_rfc3339(),
                event.event_kind.as_str(),
                event.detail,
                metadata_str,
            ],
        )?;
        Ok(())
    }

    pub fn insert_finding(
        &self,
        workgroup_id: &str,
        content: &str,
        source_task_id: Option<&str>,
    ) -> Result<()> {
        let now = Local::now().to_rfc3339();
        self.db().execute(
            "INSERT INTO findings (workgroup_id, content, source_task_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![workgroup_id, content, source_task_id, now],
        )?;
        Ok(())
    }

    pub fn insert_memory(&self, memory: &Memory) -> Result<()> {
        self.db().execute(
            "INSERT OR IGNORE INTO memories (id, memory_type, content, source_task_id, agent,
              project_path, content_hash, created_at, expires_at, supersedes, version,
              inject_count, last_injected_at, success_count)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                memory.id.as_str(),
                memory.memory_type.as_str(),
                memory.content,
                memory.source_task_id,
                memory.agent,
                memory.project_path,
                memory.content_hash,
                memory.created_at.to_rfc3339(),
                memory.expires_at.map(|dt| dt.to_rfc3339()),
                memory.supersedes.as_ref().map(|id| id.as_str()),
                memory.version,
                memory.inject_count,
                memory.last_injected_at.map(|dt| dt.to_rfc3339()),
                memory.success_count,
            ],
        )?;
        Ok(())
    }


    pub fn update_memory(&self, id: &str, content: &str) -> Result<bool> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(content, &mut hasher);
        let hash = format!("{:016x}", std::hash::Hasher::finish(&hasher));
        let now = chrono::Local::now().to_rfc3339();
        let conn = self.db();
        let existing = match conn.query_row(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories WHERE id = ?1",
            params![id],
            row_to_memory,
        ) {
            Ok(row) => row?,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        let Memory {
            id: old_id,
            memory_type,
            source_task_id,
            agent,
            project_path,
            expires_at,
            version,
            inject_count,
            last_injected_at,
            success_count,
            ..
        } = existing;
        let supersedes_id = old_id.as_str().to_string();
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());
        let last_injected_at_str = last_injected_at.map(|dt| dt.to_rfc3339());
        let new_id = MemoryId::generate();
        conn.execute(
            "INSERT INTO memories (id, memory_type, content, source_task_id, agent, project_path,
             content_hash, created_at, expires_at, supersedes, version, inject_count, last_injected_at,
             success_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                new_id.as_str(),
                memory_type.as_str(),
                content,
                source_task_id,
                agent,
                project_path,
                hash,
                now,
                expires_at_str,
                supersedes_id,
                version + 1,
                inject_count,
                last_injected_at_str,
                success_count,
            ],
        )?;
        Ok(true)
    }

    pub fn increment_memory_inject(&self, id: &str) -> Result<bool> {
        let now = Local::now().to_rfc3339();
        let rows = self.db().execute(
            "UPDATE memories SET inject_count = inject_count + 1, last_injected_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(rows > 0)
    }

    pub fn increment_memory_success(&self, id: &str) -> Result<bool> {
        let rows = self
            .db()
            .execute("UPDATE memories SET success_count = success_count + 1 WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        self.db()
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_verify_status(&self, id: &str, verify_status: VerifyStatus) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET verify_status = ?1 WHERE id = ?2",
            params![verify_status.as_str(), id],
        )?;
        Ok(())
    }
}
