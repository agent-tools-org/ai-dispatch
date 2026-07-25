// Dispatch guards that enforce task isolation invariants before launch.
// Exports the worktree-task repo-root guard used by run preparation.
// Deps: Task metadata plus std path canonicalization.
use anyhow::{Context, Result};
use std::path::Path;

use crate::types::Task;

pub(super) fn ensure_worktree_task_not_repo_root(
    task: &Task,
    resolved_dir: Option<&str>,
    repo_path: Option<&str>,
) -> Result<()> {
    let Some(branch) = task.worktree_branch.as_deref() else {
        return Ok(());
    };
    let Some(dir) = resolved_dir else {
        return Ok(());
    };
    let Some(repo) = repo_path.or(task.repo_path.as_deref()) else {
        return Ok(());
    };
    if !same_existing_path(Path::new(dir), Path::new(repo))? {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to launch task {} for worktree branch {} in repository root {}; resolved working directory must be an isolated worktree",
        task.id,
        branch,
        dir
    )
}

fn same_existing_path(left: &Path, right: &Path) -> Result<bool> {
    if !left.is_dir() || !right.is_dir() {
        return Ok(false);
    }
    let left = left
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", left.display()))?;
    let right = right
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", right.display()))?;
    Ok(left == right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};

    fn task_with_branch(id: &str, branch: Option<&str>) -> Task {
        Task {
            id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
            prompt: "prompt".to_string(), resolved_prompt: None, category: None,
            status: TaskStatus::Pending, parent_task_id: None, workgroup_id: None,
            caller_kind: None, caller_session_id: None, agent_session_id: None,
            repo_path: None, worktree_path: None, worktree_branch: branch.map(str::to_string),
            final_head_sha: None, final_branch: None, start_sha: None, log_path: None,
            output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
            model: None, cost_usd: None, exit_code: None, created_at: Local::now(),
            completed_at: None, verify: None, verify_status: VerifyStatus::Skipped,
            pending_reason: None, read_only: false, budget: false, audit_verdict: None,
            audit_report_path: None, delivery_assessment: None,
        }
    }

    #[test]
    fn guard_rejects_worktree_task_resolved_to_repo_root() {
        let repo = tempfile::tempdir().unwrap();
        let task = task_with_branch("t-guard", Some("feat/guard"));
        let repo_path = repo.path().to_string_lossy().to_string();

        let err = ensure_worktree_task_not_repo_root(&task, Some(repo_path.as_str()), Some(repo_path.as_str()))
            .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("t-guard"));
        assert!(message.contains("feat/guard"));
        assert!(message.contains(&repo_path));
    }

    #[test]
    fn guard_allows_non_worktree_task_in_repo_root() {
        let repo = tempfile::tempdir().unwrap();
        let task = task_with_branch("t-main", None);
        let repo_path = repo.path().to_string_lossy().to_string();

        ensure_worktree_task_not_repo_root(&task, Some(repo_path.as_str()), Some(repo_path.as_str())).unwrap();
    }

    #[test]
    fn guard_allows_worktree_task_in_distinct_dir() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        let task = task_with_branch("t-wt", Some("feat/wt"));
        let repo_path = repo.path().to_string_lossy().to_string();
        let worktree_path = worktree.path().to_string_lossy().to_string();

        ensure_worktree_task_not_repo_root(
            &task,
            Some(worktree_path.as_str()),
            Some(repo_path.as_str()),
        )
        .unwrap();
    }
}
