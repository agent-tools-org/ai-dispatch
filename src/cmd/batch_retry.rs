// Batch retry logic for failed/skipped tasks.
// Exports: retry_failed
// Deps: crate::cmd::run, crate::store::Store, crate::types::Task
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::{Task, TaskStatus};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};
const SERIAL_RETRY_POLL_SECS: u64 = 2;
const SERIAL_RETRY_TIMEOUT_SECS: u64 = 30 * 60;
type WorktreeIdentity = (Option<String>, Option<String>);
#[derive(Debug)]
struct RetryBucket {
    worktree_path: Option<String>,
    worktree_branch: Option<String>,
    tasks: Vec<Task>,
}
impl RetryBucket {
    fn new(task: Task) -> Self { Self { worktree_path: task.worktree_path.clone(), worktree_branch: task.worktree_branch.clone(), tasks: vec![task] } }
    fn label(&self) -> String {
        self.worktree_branch
            .clone()
            .or_else(|| self.worktree_path.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

pub async fn retry_failed(
    store: Arc<Store>,
    group_id: &str,
    agent_override: Option<&str>,
    include_waiting: bool,
) -> Result<()> {
    crate::sanitize::validate_workgroup_id(group_id)?;
    let tasks = store.list_tasks_by_group(group_id)?;
    let total = tasks.len();
    let retry_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| should_retry_task(task.status, include_waiting))
        .collect();
    if retry_tasks.is_empty() {
        println!("No retryable tasks in {group_id}");
        return Ok(());
    }
    println!("[batch] Retrying {}/{} task(s) in {group_id}", retry_tasks.len(), total);
    for bucket in bucket_retry_tasks(retry_tasks) {
        dispatch_retry_bucket(&store, bucket, group_id, agent_override).await?;
    }
    Ok(())
}

pub(super) fn should_retry_task(status: TaskStatus, include_waiting: bool) -> bool {
    matches!(status, TaskStatus::Failed | TaskStatus::Skipped)
        || (include_waiting && status == TaskStatus::Waiting)
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

fn bucket_retry_tasks(tasks: Vec<Task>) -> Vec<RetryBucket> {
    let mut buckets = Vec::new();
    let mut bucket_indexes = HashMap::<WorktreeIdentity, usize>::new();
    for task in tasks {
        let Some(identity) = retry_bucket_identity(&task) else { buckets.push(RetryBucket::new(task)); continue; };
        if let Some(bucket_idx) = bucket_indexes.get(&identity).copied() {
            buckets[bucket_idx].tasks.push(task);
            continue;
        }
        let bucket_idx = buckets.len();
        bucket_indexes.insert(identity, bucket_idx);
        buckets.push(RetryBucket::new(task));
    }
    buckets
}

fn retry_bucket_identity(task: &Task) -> Option<WorktreeIdentity> { if task.worktree_path.is_none() && task.worktree_branch.is_none() { None } else { Some((task.worktree_path.clone(), task.worktree_branch.clone())) } }

async fn dispatch_retry_bucket(
    store: &Arc<Store>,
    bucket: RetryBucket,
    group_id: &str,
    agent_override: Option<&str>,
) -> Result<()> {
    if bucket.tasks.len() == 1 {
        let run_args = retry_task_to_run_args(&bucket.tasks[0], group_id, agent_override);
        let _ = run::run(store.clone(), run_args).await?;
        return Ok(());
    }
    let label = bucket.label();
    println!("[aid] Serializing {} retries in worktree {}", bucket.tasks.len(), label);
    for task in bucket.tasks {
        let run_args = retry_task_to_run_args(&task, group_id, agent_override);
        let task_id = run::run(store.clone(), run_args).await?;
        wait_for_retry_completion(store, task_id.as_str()).await?;
    }
    println!("[aid] Worktree {} bucket complete", label);
    Ok(())
}

async fn wait_for_retry_completion(store: &Arc<Store>, task_id: &str) -> Result<()> {
    let timeout = Duration::from_secs(SERIAL_RETRY_TIMEOUT_SECS);
    let deadline = Instant::now() + timeout;
    loop {
        let _ = crate::background::check_zombie_tasks(store);
        if let Some(task) = store.get_task(task_id)? && task.status.is_terminal() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("Timed out waiting for retried task {} after {}s", task_id, SERIAL_RETRY_TIMEOUT_SECS);
        }
        sleep(Duration::from_secs(SERIAL_RETRY_POLL_SECS)).await;
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
            category: None,
            status: TaskStatus::Failed,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
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
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    fn task_ids(bucket: &RetryBucket) -> Vec<String> { bucket.tasks.iter().map(|task| task.id.to_string()).collect() }
    fn aid_home_guard(name: &str) -> paths::AidHomeGuard {
        let temp_dir = std::env::temp_dir().join(name);
        let guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();
        guard
    }

    #[test]
    fn retry_uses_original_when_not_rate_limited() {
        let _guard = aid_home_guard("aid-retry-fallback-test-normal");
        let task = make_task("t-001", AgentKind::Codex);
        let args = retry_task_to_run_args(&task, "wg-test", None);
        assert_eq!(args.agent_name, "codex");
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn retry_uses_fallback_when_rate_limited() {
        let _guard = aid_home_guard("aid-retry-fallback-test-limited");
        // CI hosts have no agent binaries on PATH, so pin the detected set
        // to exercise the fallback logic deterministically.
        let _agents = crate::agent::DetectAgentsGuard::set(vec![
            AgentKind::Gemini,
            AgentKind::Qwen,
            AgentKind::Codex,
            AgentKind::Copilot,
        ]);
        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        let task = make_task("t-002", AgentKind::Codex);
        let args = retry_task_to_run_args(&task, "wg-test", None);
        assert_ne!(args.agent_name, "codex", "Should use fallback when rate-limited");
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn retry_uses_override_regardless_of_rate_limit() {
        let _guard = aid_home_guard("aid-retry-fallback-test-override");
        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        let task = make_task("t-003", AgentKind::Codex);
        let args = retry_task_to_run_args(&task, "wg-test", Some("gemini"));
        assert_eq!(args.agent_name, "gemini", "Override should bypass rate limit check");
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn retry_unchanged_for_unknown_agent() {
        let _guard = aid_home_guard("aid-retry-fallback-test-unknown");
        let task = make_task("t-004", AgentKind::Custom);
        let args = retry_task_to_run_args(&task, "wg-test", None);
        assert_eq!(args.agent_name, "custom");
    }

    #[test]
    fn retry_bucket_groups_shared_worktree_tasks() {
        let mut first = make_task("t-101", AgentKind::Codex);
        let mut second = make_task("t-102", AgentKind::Codex);
        let mut third = make_task("t-103", AgentKind::Codex);
        first.worktree_branch = Some("feat/gitbutler".to_string());
        second.worktree_branch = Some("feat/gitbutler".to_string());
        third.worktree_branch = Some("feat/gitbutler".to_string());
        let buckets = bucket_retry_tasks(vec![first, second, third]);
        assert_eq!(buckets.len(), 1);
        assert_eq!(task_ids(&buckets[0]), vec!["t-101".to_string(), "t-102".to_string(), "t-103".to_string()]);
    }

    #[test]
    fn retry_bucket_separates_distinct_worktrees() {
        let mut first = make_task("t-201", AgentKind::Codex);
        let mut second = make_task("t-202", AgentKind::Codex);
        first.worktree_branch = Some("feat/a".to_string());
        second.worktree_branch = Some("feat/b".to_string());
        let buckets = bucket_retry_tasks(vec![first, second]);
        assert_eq!(buckets.len(), 2);
        assert_eq!(task_ids(&buckets[0]), vec!["t-201".to_string()]);
        assert_eq!(task_ids(&buckets[1]), vec!["t-202".to_string()]);
    }

    #[test]
    fn retry_bucket_treats_none_worktree_as_unique() {
        let first = make_task("t-301", AgentKind::Codex);
        let second = make_task("t-302", AgentKind::Codex);
        let third = make_task("t-303", AgentKind::Codex);
        let buckets = bucket_retry_tasks(vec![first, second, third]);
        assert_eq!(buckets.len(), 3);
        assert_eq!(task_ids(&buckets[0]), vec!["t-301".to_string()]);
        assert_eq!(task_ids(&buckets[1]), vec!["t-302".to_string()]);
        assert_eq!(task_ids(&buckets[2]), vec!["t-303".to_string()]);
    }
}
