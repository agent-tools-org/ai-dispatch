// Task execution, audit, verification and delivery metadata writes.
// Deps: Store, SQLite parameters and task record types.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::types::*;

impl Store {
    pub fn update_resolved_prompt(&self, id: &str, resolved_prompt: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET resolved_prompt = ?1 WHERE id = ?2",
            params![resolved_prompt, id],
        )?;
        Ok(())
    }

    pub fn update_task_dispatch_args(&self, id: &str, dispatch_args: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET dispatch_args = ?1 WHERE id = ?2",
            params![dispatch_args, id],
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

    pub(crate) fn update_task_worktree(
        &self,
        id: &str,
        repo_path: Option<&str>,
        worktree_path: Option<&str>,
        worktree_branch: Option<&str>,
        effective_dir: Option<&str>,
    ) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET repo_path = ?1, worktree_path = ?2, worktree_branch = ?3, effective_dir = ?4 WHERE id = ?5",
            params![repo_path, worktree_path, worktree_branch, effective_dir, id],
        )?;
        Ok(())
    }

    pub(crate) fn update_task_project_id(&self, id: &str, project_id: Option<&str>) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET project_id = ?1 WHERE id = ?2",
            params![project_id, id],
        )?;
        Ok(())
    }

    pub fn update_start_sha(&self, id: &str, start_sha: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET start_sha = ?1 WHERE id = ?2",
            params![start_sha, id],
        )?;
        Ok(())
    }

    pub(crate) fn update_task_final_state(
        &self,
        id: &str,
        final_head_sha: Option<&str>,
        final_branch: Option<&str>,
    ) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET final_head_sha = ?1, final_branch = ?2 WHERE id = ?3",
            params![final_head_sha, final_branch, id],
        )?;
        Ok(())
    }

    pub fn update_output_path(&self, task_id: &str, output_path: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET output_path = ?1 WHERE id = ?2",
            params![output_path, task_id],
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

    pub fn save_completion_summary(&self, task_id: &str, summary_json: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET completion_summary = ?1 WHERE id = ?2",
            params![summary_json, task_id],
        )?;
        Ok(())
    }

    pub fn update_task_audit(
        &self,
        task_id: &str,
        verdict: Option<&str>,
        report_path: Option<&str>,
    ) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET audit_verdict = ?1, audit_report_path = ?2 WHERE id = ?3",
            params![verdict, report_path, task_id],
        )?;
        Ok(())
    }

    pub fn save_peer_review(
        &self,
        task_id: &str,
        reviewer: &str,
        score: u8,
        feedback: &str,
    ) -> Result<()> {
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

    pub fn update_verify_status(&self, id: &str, verify_status: VerifyStatus) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET verify_status = ?1 WHERE id = ?2",
            params![verify_status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn update_delivery_assessment(
        &self,
        id: &str,
        delivery_assessment: Option<DeliveryAssessment>,
    ) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET delivery_assessment = ?1 WHERE id = ?2",
            params![delivery_assessment.map(|value| value.as_str()), id],
        )?;
        Ok(())
    }

    pub fn update_principal_merge_override(&self, id: &str, reason: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET principal_merge_override = ?1 WHERE id = ?2",
            params![reason, id],
        )?;
        Ok(())
    }

    pub fn get_principal_merge_override(&self, id: &str) -> Result<Option<String>> {
        let reason = self
            .db()
            .query_row(
                "SELECT principal_merge_override FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(reason)
    }}
