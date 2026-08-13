// Tests for `aid retry` replay-directory resolution.
// When the persisted dispatch dir is null/empty/'.', a retry must replay the
// task's absolute effective directory (falling back to its repo) rather than
// the process cwd of the invocation, and must refuse when neither is usable.
// Deps: retry_task_to_run_args, RetryArgs, RunArgs, Store.

use super::{retry_task_to_run_args, RetryArgs};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

fn task_with(id: &str, effective_dir: Option<String>, repo_path: Option<String>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Qwen,
        custom_agent_name: None,
        prompt: "original prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: Some("ec9e3217-6444-424c-b21a-1d026d22928d".to_string()),
        repo_path: repo_path.clone(),
        project_id: None,
        worktree_path: None,
        effective_dir,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
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

fn base_retry(task_id: &str) -> RetryArgs {
    RetryArgs {
        task_id: task_id.to_string(),
        feedback: Some("fix it".to_string()),
        feedback_file: None,
        agent: None,
        model: None,
        idle_timeout_secs: None,
        dir: None,
        reset: false,
        bg: false,
    }
}

fn insert_with_saved(store: &Store, task: &Task, saved: &RunArgs) {
    store.insert_task(task).unwrap();
    store
        .update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap())
        .unwrap();
}

#[test]
fn retry_repo_without_dir_replays_repo_not_process_cwd() {
    let store = Store::open_memory().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path().to_string_lossy().to_string();
    let task = task_with("t-repo-no-dir", Some(repo_path.clone()), Some(repo_path.clone()));
    let saved = RunArgs {
        agent_name: "qwen".to_string(),
        prompt: task.prompt.clone(),
        repo: Some(repo_path.clone()),
        dir: None,
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let run_args = retry_task_to_run_args(&store, &task, base_retry(task.id.as_str()), false).unwrap();

    assert_eq!(
        run_args.dir.as_deref(),
        Some(repo_path.as_str()),
        "a --repo dispatch without --dir must replay the repo, not the invocation cwd"
    );
}

#[test]
fn retry_missing_recorded_dir_falls_back_to_repo() {
    let store = Store::open_memory().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path().to_string_lossy().to_string();
    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().to_string_lossy().to_string();
    gone.close().unwrap();
    let task = task_with("t-gone-dir", Some(gone_path.clone()), Some(repo_path.clone()));
    let saved = RunArgs {
        agent_name: "qwen".to_string(),
        prompt: task.prompt.clone(),
        dir: None,
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let run_args = retry_task_to_run_args(&store, &task, base_retry(task.id.as_str()), false).unwrap();

    assert_ne!(run_args.dir.as_deref(), Some(gone_path.as_str()));
    assert_eq!(
        run_args.dir.as_deref(),
        Some(repo_path.as_str()),
        "a missing recorded directory must fall back to the task's repo"
    );
}

#[test]
fn retry_without_dir_replays_absolute_effective_dir() {
    let store = Store::open_memory().unwrap();
    let original = tempfile::tempdir().unwrap();
    let original_path = original.path().to_string_lossy().to_string();
    let task = task_with("t-plain", Some(original_path.clone()), None);
    let saved = RunArgs {
        agent_name: "qwen".to_string(),
        prompt: task.prompt.clone(),
        dir: None,
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let run_args = retry_task_to_run_args(&store, &task, base_retry(task.id.as_str()), false).unwrap();

    assert_eq!(
        run_args.dir.as_deref(),
        Some(original_path.as_str()),
        "a plain dispatch must replay the same absolute directory the first run used"
    );
}

#[test]
fn retry_refuses_when_recorded_dir_and_repo_are_unusable() {
    let store = Store::open_memory().unwrap();
    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().to_string_lossy().to_string();
    gone.close().unwrap();
    let task = task_with("t-refuse", Some(gone_path), None);
    let saved = RunArgs {
        agent_name: "qwen".to_string(),
        prompt: task.prompt.clone(),
        dir: None,
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let err = match retry_task_to_run_args(&store, &task, base_retry(task.id.as_str()), false) {
        Ok(_) => panic!("retry with no usable directory should have been refused"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("refusing to guess"),
        "error should refuse loudly, got: {err}"
    );
}
