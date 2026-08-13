// Tests for `aid retry` model/idle-timeout/feedback-file overrides.
// Unspecified flags must inherit the original task's saved values.
// Deps: retry_task_to_run_args, RetryArgs, RunArgs, Store.

use super::{retry_task_to_run_args, RetryArgs};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

fn failed_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "original prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: Some(std::env::temp_dir().to_string_lossy().to_string()),
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: Some("gpt-saved".to_string()),
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
fn retry_model_override_replaces_saved_model() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-model-override");
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: task.prompt.clone(),
        model: Some("gpt-saved".to_string()),
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let mut args = base_retry(task.id.as_str());
    args.model = Some("gpt-retry".to_string());
    let run_args = retry_task_to_run_args(&store, &task, args, false).unwrap();

    assert_eq!(run_args.model.as_deref(), Some("gpt-retry"));
}

#[test]
fn retry_unspecified_model_inherits_original() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-model-inherit");
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: task.prompt.clone(),
        model: Some("gpt-saved".to_string()),
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let run_args = retry_task_to_run_args(&store, &task, base_retry(task.id.as_str()), false).unwrap();

    assert_eq!(
        run_args.model.as_deref(),
        Some("gpt-saved"),
        "unspecified --model must inherit the original task model, not a default"
    );
}

#[test]
fn retry_idle_timeout_override_replaces_saved() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-idle-override");
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: task.prompt.clone(),
        idle_timeout_secs: Some(300),
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let mut args = base_retry(task.id.as_str());
    args.idle_timeout_secs = Some(900);
    let run_args = retry_task_to_run_args(&store, &task, args, false).unwrap();

    assert_eq!(run_args.idle_timeout_secs, Some(900));
}

#[test]
fn retry_unspecified_idle_timeout_inherits_original() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-idle-inherit");
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: task.prompt.clone(),
        idle_timeout_secs: Some(420),
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);

    let run_args = retry_task_to_run_args(&store, &task, base_retry(task.id.as_str()), false).unwrap();

    assert_eq!(
        run_args.idle_timeout_secs,
        Some(420),
        "unspecified --idle-timeout must inherit the original task value, not a global default"
    );
}

#[test]
fn retry_feedback_file_loads_contents() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-feedback-file");
    store.insert_task(&task).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("feedback.md");
    std::fs::write(&path, "long feedback from file").unwrap();

    let mut args = base_retry(task.id.as_str());
    args.feedback = None;
    args.feedback_file = Some(path.to_string_lossy().to_string());
    let run_args = retry_task_to_run_args(&store, &task, args, false).unwrap();

    assert!(
        run_args.prompt.contains("long feedback from file"),
        "prompt should include feedback file contents, got: {}",
        run_args.prompt
    );
}

#[test]
fn retry_rejects_both_feedback_and_feedback_file() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-feedback-conflict");
    store.insert_task(&task).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("feedback.md");
    std::fs::write(&path, "from file").unwrap();

    let mut args = base_retry(task.id.as_str());
    args.feedback = Some("inline".to_string());
    args.feedback_file = Some(path.to_string_lossy().to_string());
    let err = match retry_task_to_run_args(&store, &task, args, false) {
        Ok(_) => panic!("conflicting feedback sources were accepted"),
        Err(err) => err,
    };

    let msg = err.to_string();
    assert!(
        msg.contains("--feedback") && msg.contains("--feedback-file"),
        "error should name both flags, got: {msg}"
    );
}

#[test]
fn retry_rejects_missing_feedback_sources() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-feedback-missing");
    store.insert_task(&task).unwrap();

    let mut args = base_retry(task.id.as_str());
    args.feedback = None;
    args.feedback_file = None;
    let err = match retry_task_to_run_args(&store, &task, args, false) {
        Ok(_) => panic!("missing feedback was accepted"),
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("--feedback"),
        "error should require feedback, got: {}",
        err
    );
}
