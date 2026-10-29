// Tests for failed-task auto-retry worktree target recovery.
// Covers pruned worktree directories, branch-tip recreation, and fail-closed errors.
// Deps: retry_logic builder, Store, git CLI, worktree helpers.

use super::*;





use crate::support::test_env::git;

use crate::support::test_env::git_output;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("base.txt", "base\n", "init")
}

fn failed_task(id: &str) -> Task {
    crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Failed)
}

fn retry_args_for(store: &Store, task: &Task) -> Result<RunArgs> {
    build_failed_retry_args(
        store,
        &task.id,
        &RunArgs { retry: 1, prompt: "fallback prompt".to_string(), ..Default::default() },
        task,
        "agent failed",
    )
}

#[test]
fn failed_retry_with_pruned_worktree_keeps_branch_target() {
    let _permit = crate::support::test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let repo = init_repo();
    let branch = format!("fix/retry-pruned-{}", std::process::id());
    git(repo.path(), &["branch", &branch]);
    let missing = repo.path().join("missing-worktree");
    let mut task = failed_task("t-pruned-retry");
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(missing.display().to_string());
    task.worktree_branch = Some(branch.clone());
    store.insert_task(&task).unwrap();

    let retry_args = retry_args_for(&store, &task).unwrap();

    assert_eq!(retry_args.worktree.as_deref(), Some(branch.as_str()));
    assert_eq!(retry_args.dir.as_deref(), Some(repo.path().to_str().unwrap()));
    assert_eq!(retry_args.base_branch.as_deref(), Some(branch.as_str()));
}

#[test]
fn pruned_retry_recreates_worktree_from_parent_branch_tip() {
    let _permit = crate::support::test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let repo = init_repo();
    let branch = format!("fix/retry-preserve-{}", std::process::id());
    let worktree = crate::workspace::worktree::create_worktree(repo.path(), &branch, Some("main")).unwrap();
    std::fs::write(worktree.path.join("sentinel.txt"), "keep\n").unwrap();
    git(&worktree.path, &["add", "sentinel.txt"]);
    git(&worktree.path, &["commit", "-m", "parent commit"]);
    let parent_head = git_output(repo.path(), &["rev-parse", &branch]);
    git(repo.path(), &["worktree", "remove", "--force", &worktree.path.to_string_lossy()]);
    let mut task = failed_task("t-recreate-retry");
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(worktree.path.display().to_string());
    task.worktree_branch = Some(branch.clone());
    store.insert_task(&task).unwrap();

    let retry_args = retry_args_for(&store, &task).unwrap();
    let recreated = crate::workspace::worktree::create_worktree(
        repo.path(),
        retry_args.worktree.as_deref().unwrap(),
        retry_args.base_branch.as_deref(),
    )
    .unwrap();

    assert_eq!(git_output(repo.path(), &["rev-parse", &branch]), parent_head);
    assert_eq!(std::fs::read_to_string(recreated.path.join("sentinel.txt")).unwrap(), "keep\n");
}

#[test]
fn failed_retry_errors_when_recorded_branch_is_gone() {
    let _permit = crate::support::test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let repo = init_repo();
    let branch = format!("fix/retry-missing-{}", std::process::id());
    let mut task = failed_task("t-unrecoverable-retry");
    task.repo_path = Some(repo.path().display().to_string());
    task.worktree_path = Some(repo.path().join("missing-worktree").display().to_string());
    task.worktree_branch = Some(branch.clone());
    store.insert_task(&task).unwrap();

    let message = match retry_args_for(&store, &task) {
        Ok(_) => panic!("retry unexpectedly recovered missing branch"),
        Err(err) => err.to_string(),
    };

    assert!(message.contains("t-unrecoverable-retry"));
    assert!(message.contains(&branch));
    assert!(message.contains("branch does not exist"));
    assert!(message.contains("refusing to run in repo root"));
}

#[test]
fn failed_retry_keeps_saved_configuration_and_uses_own_feedback() {
    let store = Store::open_memory().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let mut root = failed_task("t-config-root");
    root.prompt = "root prompt".into();
    store.insert_task(&root).unwrap();
    let mut child = failed_task("t-config-child");
    child.parent_task_id = Some(root.id.to_string());
    child.repo_path = Some(repo.path().display().to_string());
    child.output_path = Some("row-output".into());
    store.insert_task(&child).unwrap();
    let saved = RunArgs {
        verify: Some("saved-verify".into()), output: Some("saved-output".into()),
        budget: true, read_only: true, on_done: Some("saved-hook".into()),
        ..Default::default()
    };
    store.update_task_dispatch_args(child.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();
    let args = retry_args_for(&store, &child).unwrap();
    assert_eq!(args.prompt, "[Previous attempt failed]\nError: agent failed\n\n[Original task]\nprompt");
    assert_eq!(args.verify, saved.verify);
    assert_eq!(args.output, saved.output);
    assert_eq!(args.on_done, saved.on_done);
    assert!(args.budget && args.read_only);
    assert_eq!(args.retry, 0);
    assert_eq!(args.parent_task_id.as_deref(), Some(child.id.as_str()));
    assert!(!args.background);
}

#[test]
fn retry_preservation_execution_failure_keeps_distinct_child_brief() {
    assert_failed_retry_preserves("Implement child B, not parent A");
}

#[test]
fn retry_preservation_execution_failure_keeps_checklist_feedback() {
    assert_failed_retry_preserves(
        concat!("[Checklist items not addressed]\nYou MUST address these items:\n",
            "Missing item\n\n[Original task]\nChild brief"),
    );
}

#[test]
fn retry_preservation_execution_failure_keeps_iteration_feedback() {
    assert_failed_retry_preserves(
        "[Iteration feedback]\nFix evaluation failure\n\n[Original task]\nChild brief",
    );
}

fn assert_failed_retry_preserves(brief: &str) {
    let store = Store::open_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut parent = failed_task("t-parent-preservation");
    parent.prompt = "Unrelated ancestor A".into();
    store.insert_task(&parent).unwrap();
    let mut child = failed_task("t-child-preservation");
    child.prompt = brief.repeat(500);
    child.repo_path = Some(dir.path().display().to_string());
    child.resolved_prompt = Some("Auxiliary launch context must not be inherited".into());
    child.parent_task_id = Some(parent.id.to_string());
    store.insert_task(&child).unwrap();
    let retry = retry_args_for(&store, &child).unwrap();
    assert_eq!(
        retry.prompt,
        format!(
            "[Previous attempt failed]\nError: agent failed\n\n[Original task]\n{}",
            child.prompt,
        )
    );
    assert!(!retry.prompt.contains("Unrelated ancestor A"));
    assert!(!retry.prompt.contains("Auxiliary launch context"));
}
