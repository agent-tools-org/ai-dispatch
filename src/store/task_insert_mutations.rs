// Task insertion and conditional replacement of waiting dispatch rows.
// Deps: Store, task records and rusqlite parameters.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use super::Store;
use crate::types::*;

impl Store {
    pub fn insert_task(&self, task: &Task) -> Result<()> {
        let agent_value = if task.agent == AgentKind::Custom {
            task.custom_agent_name.as_deref().unwrap_or("custom")
        } else {
            task.agent.as_str()
        };
        let conn = self.db();
        let mut statement = conn.prepare(
            "INSERT INTO tasks (id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, project_id, worktree_path, worktree_branch,
             final_head_sha, final_branch, start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd, exit_code,
             created_at, completed_at, verify, verify_status, read_only, budget, custom_agent_name,
             category, pending_reason, audit_verdict, audit_report_path, delivery_assessment, observed_model, attribution_source, effective_dir)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
             ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34,
             ?35, ?36, ?37, ?38, ?39, ?40
             WHERE NOT EXISTS (SELECT 1 FROM tasks WHERE id = ?6 AND status = 'stopped')",
        )?;
        if insert_task_row(&mut statement, task, agent_value)? == 0 {
            anyhow::bail!(
                "Parent task '{}' stopped; refusing child '{}'",
                task.parent_task_id.as_deref().unwrap_or_default(),
                task.id
            );
        }
        Ok(())
    }

    /// Insert a waiting task placeholder with enough config for faithful retry.
    pub fn insert_waiting_task(
        &self,
        id: &str,
        agent: &str,
        prompt: &str,
        resolved_prompt: Option<&str>,
        workgroup_id: Option<&str>,
        dir: Option<&str>,
        worktree_branch: Option<&str>,
        model: Option<&str>,
        verify: Option<&str>,
        read_only: bool,
        budget: bool,
    ) -> Result<()> {
        self.db().execute(
            "INSERT INTO tasks (
             id, agent, prompt, resolved_prompt, status, workgroup_id, repo_path,
             worktree_branch, model, created_at, verify, verify_status, read_only, budget
             ) VALUES (?1, ?2, ?3, ?4, 'waiting', ?5, ?6, ?7, ?8, ?9, ?10, 'skipped', ?11, ?12)",
            params![
                id,
                agent,
                prompt,
                resolved_prompt,
                workgroup_id,
                dir,
                worktree_branch,
                model,
                Local::now().to_rfc3339(),
                verify,
                read_only,
                budget,
            ],
        )?;
        Ok(())
    }

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
             final_head_sha=?15, final_branch=?16, start_sha=?17, log_path=?18, output_path=?19, model=?20, verify=?21,
             verify_status=?22, read_only=?23, budget=?24, custom_agent_name=?25,
             category=?26, pending_reason=?27, audit_verdict=?28, audit_report_path=?29,
             delivery_assessment=?30, effective_dir=?31
             WHERE id=?1 AND status='waiting'
             AND NOT EXISTS (SELECT 1 FROM tasks WHERE id = ?6 AND status = 'stopped')",
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
            anyhow::bail!("Task '{id}' is no longer waiting or parent '{}' stopped; refusing replacement",
                task.parent_task_id.as_deref().unwrap_or_default());
        }
        Ok(())
    }
}

fn insert_task_row(
    statement: &mut rusqlite::Statement<'_>,
    task: &Task,
    agent_value: &str,
) -> rusqlite::Result<usize> {
    statement.execute(params![
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
        task.project_id,
        task.worktree_path,
        task.worktree_branch,
        task.final_head_sha,
        task.final_branch,
        task.start_sha,
        task.log_path,
        task.output_path,
        task.tokens,
        task.prompt_tokens,
        task.duration_ms,
        task.requested_model,
        task.cost_usd,
        task.exit_code,
        task.created_at.to_rfc3339(),
        task.completed_at.map(|t| t.to_rfc3339()),
        task.verify,
        task.verify_status.as_str(),
        task.read_only,
        task.budget,
        task.custom_agent_name,
        task.category,
        task.pending_reason,
        task.audit_verdict,
        task.audit_report_path,
        task.delivery_assessment.map(|value| value.as_str()),
        task.observed_model,
        task.attribution_source.map(|value| value.as_str()),
        task.effective_dir,
    ])
}
