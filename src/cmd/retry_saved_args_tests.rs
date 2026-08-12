// Tests for `aid retry` dispatch-args rehydration.
// Covers saved RunArgs reuse, target resolution, and CLI override precedence.
// Deps: retry builder, run::RunArgs, Store, task domain types.

use super::{retry_task_to_run_args, RetryArgs};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn linked_worktree(branch: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", branch, &linked.to_string_lossy()]);
    (repo, linked_root, linked)
}

fn failed_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "original prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: Some("wg-old".to_string()),
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None, worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None, log_path: None,
        output_path: None, tokens: None, prompt_tokens: None, duration_ms: None, requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn retry_rehydrates_saved_context_scope_team_and_agent_override_wins() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-retry");
    store.insert_task(&task).unwrap();
    let mut env = HashMap::new();
    env.insert("TOKEN".to_string(), "secret".to_string());
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "original prompt".to_string(),
        context: vec!["src/lib.rs".to_string()],
        scope: vec!["src/**".to_string()],
        team: Some("dev".to_string()),
        env: Some(env),
        ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let args = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("fix it".to_string()),
            feedback_file: None,
            agent: Some("gemini".to_string()),
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    ).unwrap();

    assert_eq!(args.agent_name, "gemini");
    assert_eq!(args.context, vec!["src/lib.rs".to_string()]);
    assert_eq!(args.scope, vec!["src/**".to_string()]);
    assert_eq!(args.team, Some("dev".to_string()));
    assert_eq!(args.parent_task_id, Some("t-retry".to_string()));
    assert!(args.env.is_none());
}

#[test]
fn retry_without_worktree_preserves_saved_dir() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-saved-dir");
    let saved_dir = "/tmp/original-manual-worktree";
    insert_task_with_saved_dir(&store, &task, saved_dir);

    let args = retry_args(&store, &task, None);

    assert_eq!(args.dir.as_deref(), Some(saved_dir));
    assert!(args.worktree.is_none());
}

#[test]
fn retry_dir_override_wins_over_existing_worktree() {
    let store = Store::open_memory().unwrap();
    let worktree = tempfile::tempdir().unwrap();
    let mut task = failed_task("t-dir-override");
    task.worktree_path = Some(worktree.path().to_string_lossy().to_string());
    task.worktree_branch = Some("aid/existing".to_string());
    insert_task_with_saved_dir(&store, &task, "/tmp/original-dir");

    let args = retry_args(&store, &task, Some("/tmp/retry-override"));

    assert_eq!(args.dir.as_deref(), Some("/tmp/retry-override"));
    assert!(args.worktree.is_none());
}

#[test]
fn retry_existing_worktree_overrides_saved_dir() {
    let store = Store::open_memory().unwrap();
    let (repo, _linked_root, worktree) = linked_worktree("aid/existing");
    let repo_path = repo.path().to_string_lossy().to_string();
    let worktree_path = worktree.to_string_lossy().to_string();
    let mut task = failed_task("t-existing-worktree");
    task.repo_path = Some(repo_path.clone());
    task.worktree_path = Some(worktree_path.clone());
    task.worktree_branch = Some("aid/existing".to_string());
    insert_task_with_saved_dir(&store, &task, "/tmp/original-repo");

    let args = retry_args(&store, &task, None);
    let reused = crate::worktree::create_worktree(
        Path::new(args.repo.as_deref().expect("retry repo anchor")),
        args.worktree.as_deref().expect("retry worktree branch"),
        None,
    ).unwrap();

    assert_eq!(args.repo.as_deref(), Some(repo_path.as_str()));
    assert_eq!(args.dir.as_deref(), Some(worktree_path.as_str()));
    assert_eq!(args.worktree.as_deref(), Some("aid/existing"));
    assert_eq!(reused.path.canonicalize().unwrap(), worktree.canonicalize().unwrap());
}

#[test]
fn retry_refuses_branch_checked_out_in_dispatching_checkout() {
    let store = Store::open_memory().unwrap();
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    git(repo.path(), &["checkout", "-b", "aid/caller-branch"]);
    let repo_path = repo.path().to_string_lossy().to_string();
    let mut task = failed_task("t-caller-branch");
    task.repo_path = Some(repo_path.clone());
    task.worktree_path = Some(repo.path().join("missing-worktree").display().to_string());
    task.worktree_branch = Some("aid/caller-branch".to_string());
    insert_task_with_saved_dir(&store, &task, &repo_path);

    let args = retry_args(&store, &task, None);
    let err = crate::worktree::create_worktree(
        Path::new(args.repo.as_deref().expect("retry repo anchor")),
        args.worktree.as_deref().expect("retry worktree branch"),
        None,
    ).unwrap_err();

    assert!(err.to_string().contains("main working tree"));
}

#[test]
fn retry_refuses_persisted_worktree_that_equals_repo_path() {
    let store = Store::open_memory().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path().to_string_lossy().to_string();
    let mut task = failed_task("t-poisoned-retry");
    task.repo_path = Some(repo_path.clone());
    task.worktree_path = Some(repo_path);
    task.worktree_branch = Some("chore/poisoned".to_string());
    insert_task_with_saved_dir(&store, &task, "/tmp/original-repo");

    let err = match retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("fix it".to_string()),
            feedback_file: None,
            agent: None,
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    ) {
        Ok(_) => panic!("poisoned worktree path was accepted"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("recorded worktree path"));
}


#[test]
fn retry_cleaned_worktree_preserves_saved_dir_and_recreates_branch() {
    let store = Store::open_memory().unwrap();
    let missing_worktree = tempfile::tempdir().unwrap().path().join("cleaned");
    let mut task = failed_task("t-cleaned-worktree");
    task.worktree_path = Some(missing_worktree.to_string_lossy().to_string());
    task.worktree_branch = Some("aid/recreate".to_string());
    insert_task_with_saved_dir(&store, &task, "/tmp/original-repo");

    let args = retry_args(&store, &task, None);

    assert_eq!(args.dir.as_deref(), Some("/tmp/original-repo"));
    assert_eq!(args.worktree.as_deref(), Some("aid/recreate"));
    assert_ne!(args.dir, task.worktree_path);
}

#[test]
fn retry_without_saved_args_uses_task_repo_path() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-legacy-task");
    task.repo_path = Some("/tmp/recorded-repo".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_args(&store, &task, None);

    assert_eq!(args.dir.as_deref(), Some("/tmp/recorded-repo"));
}

fn insert_task_with_saved_dir(store: &Store, task: &Task, dir: &str) {
    store.insert_task(task).unwrap();
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: task.prompt.clone(),
        dir: Some(dir.to_string()),
        ..Default::default()
    };
    store
        .update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap())
        .unwrap();
}

fn retry_args(store: &Store, task: &Task, dir: Option<&str>) -> RunArgs {
    retry_task_to_run_args(
        store,
        task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("fix it".to_string()),
            feedback_file: None,
            agent: None,
            model: None,
            idle_timeout_secs: None,
            dir: dir.map(str::to_string),
            reset: false,
            bg: false,
        },
        false,
    )
    .unwrap()
}

/// `aid retry <id> --agent <other>` must drop both model AND session_id.
/// Before this fix, session_id was set based on whether the OLD agent supports
/// session resume — it was never cleared on agent change.
#[test]
fn retry_agent_switch_clears_session_id() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-session-switch");
    // Codex supports session resume; the session is recorded at task
    // completion and stored in agent_session_id.
    task.agent = AgentKind::Codex;
    task.agent_session_id = Some("session-abc123".to_string());
    task.requested_model = Some("gpt-5".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("fix the bug".to_string()),
            feedback_file: None,
            agent: Some("gemini".to_string()), // switching to a different CLI
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .unwrap();

    assert_eq!(args.agent_name, "gemini");
    assert!(
        args.session_id.is_none(),
        "session_id must be cleared when agent changes, got {:?}",
        args.session_id
    );
    // model must also be cleared (it is route-owned, not task-owned)
    assert!(args.model.is_none(), "model must be cleared when agent changes");
}

/// A same-agent retry must still resume its session.
#[test]
fn retry_same_agent_preserves_session_id() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-session-same");
    task.agent = AgentKind::Codex;
    task.agent_session_id = Some("session-xyz789".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_args(&store, &task, None); // no --agent override → same agent

    assert_eq!(args.agent_name, "codex");
    assert_eq!(
        args.session_id.as_deref(),
        Some("session-xyz789"),
        "same-agent retry must preserve the session id"
    );
}
