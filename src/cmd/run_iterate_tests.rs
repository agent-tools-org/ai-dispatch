// Tests for iterate-loop helpers and retry scheduling behavior.
// Covers eval success/failure flow, prompt feedback, and iteration limits.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, VerifyStatus};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn done_task(id: &str, dir: &str, parent_task_id: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Write code".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: parent_task_id.map(ToString::to_string),
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(dir.to_string()),
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
        completed_at: Some(Local::now()),
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}

fn run_args(dir: &str) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Write code".to_string(),
        dir: Some(dir.to_string()),
        dry_run: true,
        iterate: Some(3),
        eval: Some("echo ok".to_string()),
        ..Default::default()
    }
}

fn init_git_repo(dir: &Path) {
    assert!(Command::new("git").args(["init"]).current_dir(dir).status().unwrap().success());
    assert!(Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn eval_success_on_first_try_returns_none() {
    let store = Arc::new(Store::open_memory().unwrap());
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    store
        .insert_task(&done_task("t-root", temp.path().to_str().unwrap(), None))
        .unwrap();

    let result = maybe_iterate(
        &store,
        &TaskId("t-root".to_string()),
        &run_args(temp.path().to_str().unwrap()),
        &IterateConfig {
            max_iterations: 3,
            eval_command: "printf 'ok'".to_string(),
            feedback_template: None,
        },
    )
    .await
    .unwrap();

    assert!(result.is_none());
    let events = store.get_events("t-root").unwrap();
    assert!(events.iter().any(|event| event.detail == "Iteration 1/3: eval passed"));
}

#[tokio::test]
async fn eval_failure_retries_with_feedback_output() {
    let store = Arc::new(Store::open_memory().unwrap());
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    store
        .insert_task(&done_task("t-root", temp.path().to_str().unwrap(), None))
        .unwrap();
    let args = RunArgs {
        eval: Some("printf 'broken'; exit 1".to_string()),
        ..run_args(temp.path().to_str().unwrap())
    };

    let retry_id = maybe_iterate(
        &store,
        &TaskId("t-root".to_string()),
        &args,
        &IterateConfig {
            max_iterations: 3,
            eval_command: "printf 'broken'; exit 1".to_string(),
            feedback_template: None,
        },
    )
    .await
    .unwrap()
    .unwrap();

    let retry_task = store.get_task(retry_id.as_str()).unwrap().unwrap();
    assert!(retry_task.prompt.contains("Iteration 2/3: eval failed."));
    assert!(retry_task.prompt.contains("broken"));
    let retry_events = store.get_events(retry_id.as_str()).unwrap();
    assert!(retry_events.iter().any(|event| event.detail == "Iteration 2/3"));
}

#[tokio::test]
async fn max_iterations_reached_stops_retrying() {
    let store = Arc::new(Store::open_memory().unwrap());
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    store
        .insert_task(&done_task("t-root", temp.path().to_str().unwrap(), None))
        .unwrap();
    insert_iteration_event(
        store.as_ref(),
        &TaskId("t-root".to_string()),
        "Iteration 3/3".to_string(),
        3,
        3,
        "scheduled",
        None,
    );

    let result = maybe_iterate(
        &store,
        &TaskId("t-root".to_string()),
        &RunArgs {
            iterate: Some(3),
            eval: Some("printf 'still broken'; exit 1".to_string()),
            ..run_args(temp.path().to_str().unwrap())
        },
        &IterateConfig {
            max_iterations: 3,
            eval_command: "printf 'still broken'; exit 1".to_string(),
            feedback_template: None,
        },
    )
    .await
    .unwrap();

    assert!(result.is_none());
    let events = store.get_events("t-root").unwrap();
    assert!(events
        .iter()
        .any(|event| event.detail == "Iteration 3/3: eval failed (exit 1), max iterations reached"));
}

#[tokio::test]
async fn feedback_template_placeholders_are_replaced() {
    let store = Arc::new(Store::open_memory().unwrap());
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    store
        .insert_task(&done_task("t-root", temp.path().to_str().unwrap(), None))
        .unwrap();

    let retry_id = maybe_iterate(
        &store,
        &TaskId("t-root".to_string()),
        &RunArgs {
            eval: Some("printf 'lint failed'; exit 1".to_string()),
            eval_feedback_template: Some(
                "Round {iteration}/{max_iterations}: {eval_output}".to_string(),
            ),
            ..run_args(temp.path().to_str().unwrap())
        },
        &IterateConfig {
            max_iterations: 4,
            eval_command: "printf 'lint failed'; exit 1".to_string(),
            feedback_template: Some(
                "Round {iteration}/{max_iterations}: {eval_output}".to_string(),
            ),
        },
    )
    .await
    .unwrap()
    .unwrap();

    let retry_task = store.get_task(retry_id.as_str()).unwrap().unwrap();
    assert!(retry_task.prompt.contains("Round 2/4: lint failed"));
}
