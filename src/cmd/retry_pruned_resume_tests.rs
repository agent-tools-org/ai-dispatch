// Regression tests for retrying tasks whose committed worktree was pruned.
// Exports: none.
// Deps: retry_task_to_run_args, Store, git CLI, tempfile.

use super::{RetryArgs, retry_task_to_run_args};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus};

use std::path::Path;

use crate::support::test_env::git;

use crate::support::test_env::git_output;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("base.txt", "base\n", "init")
}

fn failed_task(repo: &Path, worktree: &Path, branch: &str) -> Task {
    Task { prompt: "retry prompt".to_string(),
        repo_path: Some(repo.display().to_string()), worktree_path: Some(worktree.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        ..crate::types::Task::test_fixture(TaskId("t-pruned-saved-base".to_string()), TaskStatus::Failed)
    }
}

#[test]
fn retry_recreates_pruned_committed_worktree_at_branch_tip() {
    let _permit = crate::support::test_subprocess::acquire();
    let repo = init_repo();
    let branch = format!("fix/pruned-saved-base-{}", std::process::id());
    let first = crate::workspace::worktree::create_worktree(repo.path(), &branch, Some("main")).unwrap();
    std::fs::write(first.path.join("agent.txt"), "agent\n").unwrap();
    git(&first.path, &["add", "agent.txt"]);
    git(&first.path, &["commit", "-m", "agent commit"]);
    let branch_head = git_output(repo.path(), &["rev-parse", &branch]);
    let worktree_path = first.path.clone();
    git(repo.path(), &["worktree", "remove", "--force", &worktree_path.to_string_lossy()]);

    let task = failed_task(repo.path(), &worktree_path, &branch);
    let store = Store::open_memory().unwrap();
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(repo.path().display().to_string()),
        worktree: Some(branch.clone()), base_branch: Some("main".to_string()),
        agent_name: "codex".to_string(), prompt: task.prompt.clone(), ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let retry = retry_task_to_run_args(&store, &task, RetryArgs {
        task_id: task.id.to_string(),
        feedback: Some("continue".to_string()),
        ..Default::default()
    }, false).unwrap();

    assert_eq!(retry.base_branch.as_deref(), Some(branch.as_str()));
    let recreated = crate::workspace::worktree::create_worktree(
        repo.path(), retry.worktree.as_deref().unwrap(), retry.base_branch.as_deref(),
    ).unwrap();
    assert_eq!(git_output(repo.path(), &["rev-parse", &branch]), branch_head);
    assert_eq!(git_output(&recreated.path, &["rev-parse", "HEAD"]), branch_head);
    assert_eq!(std::fs::read_to_string(recreated.path.join("agent.txt")).unwrap(), "agent\n");
}
