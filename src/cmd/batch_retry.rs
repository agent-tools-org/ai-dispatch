// Batch retry logic for failed/skipped tasks.
// Exports: retry_failed
// Deps: crate::cmd::run, crate::store::Store, crate::types::Task
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::{Task, TaskStatus};
use anyhow::Result;
use std::sync::Arc;

pub async fn retry_failed(
    store: Arc<Store>,
    group_id: &str,
    agent_override: Option<&str>,
) -> Result<()> {
    crate::sanitize::validate_workgroup_id(group_id)?;
    let tasks = store.list_tasks_by_group(group_id)?;
    let total = tasks.len();
    let retry_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Skipped))
        .collect();
    if retry_tasks.is_empty() {
        println!("No failed tasks in {group_id}");
        return Ok(());
    }
    println!(
        "[batch] Retrying {}/{} failed tasks in {group_id}",
        retry_tasks.len(),
        total
    );
    for task in retry_tasks {
        let run_args = retry_task_to_run_args(&task, group_id, agent_override);
        let _ = run::run(store.clone(), run_args).await?;
    }
    Ok(())
}

pub(crate) fn retry_task_to_run_args(task: &Task, group_id: &str, agent_override: Option<&str>) -> RunArgs {
    let (dir, worktree) = retry_target(task);
    let agent_name = if let Some(override_name) = agent_override {
        override_name.to_string()
    } else {
        let original = task.agent_display_name().to_string();
        if let Some(kind) = crate::types::AgentKind::parse_str(&original) {
            if crate::rate_limit::is_rate_limited(&kind) {
                if let Some(fallback) = crate::agent::selection::coding_fallback_for(&kind) {
                    crate::aid_info!(
                        "[aid] {} is rate-limited, retrying with fallback: {}",
                        original,
                        fallback.as_str()
                    );
                    fallback.as_str().to_string()
                } else {
                    original
                }
            } else {
                original
            }
        } else {
            original
        }
    };
    RunArgs {
        agent_name,
        prompt: task.prompt.clone(),
        repo: task.repo_path.clone(),
        dir,
        output: task.output_path.clone(),
        model: task.model.clone(),
        worktree,
        group: Some(group_id.to_string()),
        verify: task.verify.clone(),
        background: true,
        announce: true,
        parent_task_id: Some(task.id.to_string()),
        read_only: task.read_only,
        budget: task.budget,
        ..Default::default()
    }
}

fn retry_target(task: &Task) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => (Some(path.clone()), None),
        Some(_) => (None, task.worktree_branch.clone()),
        None => (task.repo_path.clone(), task.worktree_branch.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::rate_limit::{clear_rate_limit, mark_rate_limited};
    use crate::types::{AgentKind, TaskId, VerifyStatus};
    use chrono::Local;

    fn make_task(id: &str, agent: AgentKind) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
            custom_agent_name: None,
            prompt: "test prompt".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Failed,
            parent_task_id: None,
            workgroup_id: None,
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
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn retry_uses_original_when_not_rate_limited() {
        let temp_dir = std::env::temp_dir().join("aid-retry-fallback-test-normal");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();
        
        let task = make_task("t-001", AgentKind::Codex);
        let args = retry_task_to_run_args(&task, "wg-test", None);
        assert_eq!(args.agent_name, "codex");
        
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn retry_uses_fallback_when_rate_limited() {
        let temp_dir = std::env::temp_dir().join("aid-retry-fallback-test-limited");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();
        
        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        
        let task = make_task("t-002", AgentKind::Codex);
        let args = retry_task_to_run_args(&task, "wg-test", None);
        
        assert_ne!(args.agent_name, "codex", "Should use fallback when rate-limited");
        
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn retry_uses_override_regardless_of_rate_limit() {
        let temp_dir = std::env::temp_dir().join("aid-retry-fallback-test-override");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();
        
        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        
        let task = make_task("t-003", AgentKind::Codex);
        let args = retry_task_to_run_args(&task, "wg-test", Some("gemini"));
        
        assert_eq!(args.agent_name, "gemini", "Override should bypass rate limit check");
        
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn retry_unchanged_for_unknown_agent() {
        let temp_dir = std::env::temp_dir().join("aid-retry-fallback-test-unknown");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();
        
        let task = make_task("t-004", AgentKind::Custom);
        let args = retry_task_to_run_args(&task, "wg-test", None);
        
        assert_eq!(args.agent_name, "custom");
    }
}

