// Tests for batch retry logic.
// Exports: (tests only)
// Deps: super::shared + batch_retry
use super::shared::make_stored_task;
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use super::super::batch_retry::{retry_failed, retry_task_to_run_args};

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn linked_worktree(branch: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let repo = init_repo();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", branch, &linked.to_string_lossy()]);
    (repo, linked_root, linked)
}

#[test]
fn retry_task_to_run_args_uses_parent_and_original_fields() {
    let repo = init_repo();
    git(repo.path(), &["branch", "feat/retry"]);
    let mut task = make_stored_task("t-1234", AgentKind::Codex, TaskStatus::Failed);
    task.prompt = "retry me".to_string();
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_branch = Some("feat/retry".to_string());
    task.output_path = Some("out.txt".to_string());
    task.requested_model = Some("o3".to_string());
    task.verify = Some("cargo check".to_string());
    task.read_only = true;
    task.budget = true;

    let store = Store::open_memory().unwrap();
    let run_args = retry_task_to_run_args(&store, &task, "wg-batch", Some("cursor")).unwrap();

    assert_eq!(run_args.agent_name, "cursor");
    assert_eq!(run_args.prompt, "retry me");
    assert_eq!(run_args.repo, Some(repo.path().display().to_string()));
    assert_eq!(run_args.worktree, Some("feat/retry".to_string()));
    assert_eq!(run_args.verify, Some("cargo check".to_string()));
    assert_eq!(run_args.parent_task_id, Some("t-1234".to_string()));
    assert_eq!(run_args.group, Some("wg-batch".to_string()));
    assert!(run_args.background);
    assert!(run_args.read_only);
    assert!(run_args.budget);
}

#[test]
fn retry_task_to_run_args_prefers_existing_worktree_path() {
    let (repo, _linked_root, worktree) = linked_worktree("feat/retry");
    let mut task = make_stored_task("t-1234", AgentKind::Codex, TaskStatus::Failed);
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(worktree.display().to_string());
    task.worktree_branch = Some("feat/retry".to_string());

    let store = Store::open_memory().unwrap();
    let run_args = retry_task_to_run_args(&store, &task, "wg-batch", None).unwrap();

    assert_eq!(run_args.dir, task.worktree_path);
    assert_eq!(run_args.worktree, task.worktree_branch);
}

#[test]
fn retry_task_to_run_args_uses_repo_path_when_worktree_is_absent() {
    let repo = tempfile::tempdir().unwrap();
    let mut task = make_stored_task("t-no-worktree", AgentKind::Codex, TaskStatus::Failed);
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = None;
    task.worktree_branch = None;

    let store = Store::open_memory().unwrap();
    let run_args = retry_task_to_run_args(&store, &task, "wg-batch", None).unwrap();

    assert_eq!(run_args.dir, Some(repo.path().display().to_string()));
    assert_eq!(run_args.worktree, None);
}

#[test]
fn retry_task_to_run_args_errors_when_stale_worktree_has_no_repo() {
    let temp = tempfile::tempdir().unwrap();
    let stale_dir = temp.path().join("missing-worktree");
    let mut task = make_stored_task("t-stale-no-repo", AgentKind::Codex, TaskStatus::Failed);
    task.worktree_path = Some(stale_dir.display().to_string());
    task.worktree_branch = Some("feat/retry".to_string());

    let store = Store::open_memory().unwrap();
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        dir: task.worktree_path.clone(),
        worktree: task.worktree_branch.clone(),
        ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let err = retry_task_to_run_args(&store, &task, "wg-batch", None)
        .err()
        .expect("retry should fail without a usable target");

    assert!(err.to_string().contains("no recoverable repo_path"));
}

#[test]
fn retry_task_to_run_args_errors_without_worktree_or_repo() {
    let task = make_stored_task("t-no-target", AgentKind::Codex, TaskStatus::Failed);
    let store = Store::open_memory().unwrap();

    let err = retry_task_to_run_args(&store, &task, "wg-batch", None)
        .err()
        .expect("retry should fail without a usable target");

    assert!(err.to_string().contains("no usable worktree path, retry dir, or repo path"));
}

#[test]
fn retry_task_to_run_args_preserves_saved_subdir_inside_repo() {
    let repo = tempfile::tempdir().unwrap();
    let subdir = repo.path().join("crate");
    std::fs::create_dir(&subdir).unwrap();
    let mut task = make_stored_task("t-subdir", AgentKind::Codex, TaskStatus::Failed);
    task.repo_path = Some(repo.path().display().to_string());

    let store = Store::open_memory().unwrap();
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        repo: task.repo_path.clone(),
        dir: Some(subdir.display().to_string()),
        ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let run_args = retry_task_to_run_args(&store, &task, "wg-batch", None).unwrap();

    assert_eq!(run_args.dir, Some(subdir.display().to_string()));
    assert_eq!(run_args.worktree, None);
}

#[test]
fn retry_task_to_run_args_refuses_poisoned_worktree_path() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().display().to_string();
    let mut task = make_stored_task("t-poisoned-batch", AgentKind::Codex, TaskStatus::Failed);
    task.repo_path = Some(repo_path.clone());
    task.worktree_path = Some(repo_path);
    task.worktree_branch = Some("feat/retry".to_string());

    let store = Store::open_memory().unwrap();
    let err = match retry_task_to_run_args(&store, &task, "wg-batch", None) {
        Ok(_) => panic!("poisoned worktree path was accepted"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("recorded worktree path"));
}


#[test]
fn retry_task_to_run_args_rehydrates_saved_args_and_keeps_worktree() {
    let store = Store::open_memory().unwrap();
    let (repo, _linked_root, worktree) = linked_worktree("feat/retry");
    let mut task = make_stored_task("t-7777", AgentKind::Codex, TaskStatus::Failed);
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(worktree.display().to_string());
    task.worktree_branch = Some("feat/retry".to_string());
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "retry me".to_string(),
        team: Some("dev".to_string()),
        context: vec!["AGENTS.md".to_string()],
        scope: vec!["src/**".to_string()],
        ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let run_args = retry_task_to_run_args(&store, &task, "wg-batch", None).unwrap();

    assert_eq!(run_args.dir, task.worktree_path);
    assert_eq!(run_args.worktree, task.worktree_branch);
    assert_eq!(run_args.team, Some("dev".to_string()));
    assert_eq!(run_args.context, vec!["AGENTS.md".to_string()]);
    assert_eq!(run_args.scope, vec!["src/**".to_string()]);
    assert!(run_args.background);
}

#[test]
fn retry_task_to_run_args_preserves_saved_worktree_when_live_path_exists() {
    let store = Store::open_memory().unwrap();
    // Real repo + linked worktree: the isolation guard resolves the main checkout via git
    // and fails closed on paths it cannot place, so a plain temp dir will not do here.
    let (repo, _linked_root, worktree_path) = linked_worktree("feat/retry");
    let mut task = make_stored_task("t-live-saved-worktree", AgentKind::Codex, TaskStatus::Failed);
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(worktree_path.display().to_string());
    task.worktree_branch = Some("feat/retry".to_string());
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        worktree: Some("feat/retry".to_string()),
        ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let run_args = retry_task_to_run_args(&store, &task, "wg-batch", None).unwrap();

    assert_eq!(run_args.dir, task.worktree_path);
    assert_eq!(run_args.worktree, task.worktree_branch);
}

#[test]
fn retry_task_to_run_args_uses_waiting_placeholder_fields() {
    // Isolated home: without it this test reads the machine's live rate-limit
    // markers, and flips behaviour depending on whether codex happens to be
    // exhausted right now. When it was, the fallback switched the agent to agy
    // and the assertions below silently described a different code path.
    let aid_home = tempfile::tempdir().unwrap();
    let _aid_home_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let repo = init_repo();
    git(repo.path(), &["branch", "feat/retry"]);
    let prompt = "retry ".repeat(40);
    store
        .insert_waiting_task(
            "t-5678",
            "codex",
            &prompt,
            None,
            Some("wg-batch"),
            Some(repo.path().to_str().unwrap()),
            Some("feat/retry"),
            Some("o3"),
            Some("cargo check -p ai-dispatch"),
            true,
            true,
        )
        .unwrap();

    let task = store.get_task("t-5678").unwrap().unwrap();
    let run_args = retry_task_to_run_args(store.as_ref(), &task, "wg-batch", None).unwrap();

    assert_eq!(run_args.prompt, prompt);
    assert_eq!(run_args.dir, Some(repo.path().display().to_string()));
    assert_eq!(run_args.worktree, Some("feat/retry".to_string()));
    // No agent switch here, so the requested model is carried as before. The
    // cross-agent case is covered in run_cascade_tests.
    assert_eq!(run_args.model, Some("o3".to_string()));
    assert_eq!(run_args.verify, Some("cargo check -p ai-dispatch".to_string()));
    assert!(run_args.read_only);
    assert!(run_args.budget);
}

#[tokio::test]
async fn retry_failed_returns_ok_when_no_failed_tasks_exist() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_stored_task("t-1234", AgentKind::Codex, TaskStatus::Done);
    task.workgroup_id = Some("wg-batch".to_string());
    store.insert_task(&task).unwrap();

    let result = retry_failed(store, "wg-batch", None, false).await;

    assert!(result.is_ok());
}

#[test]
fn retry_filter_includes_waiting_only_when_requested() {
    use super::super::batch_retry::should_retry_task;

    assert!(should_retry_task(TaskStatus::Failed, false));
    assert!(should_retry_task(TaskStatus::Skipped, false));
    assert!(!should_retry_task(TaskStatus::Waiting, false));
    assert!(should_retry_task(TaskStatus::Waiting, true));
    assert!(!should_retry_task(TaskStatus::Running, true));
}
