// Tests for retry target selection and legacy metadata failures.
// Covers live worktree retries when persisted repo metadata is incomplete.
// Deps: run retry-target helpers, git worktrees, task domain types.

use super::*;

use crate::support::test_env::git;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("file.txt", "hello\n", "init")
}

fn failed_task(id: &str) -> Task {
    Task {
        prompt: "original prompt".to_string(),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Failed)
    }
}

#[test]
fn live_worktree_retry_without_repo_path_derives_repo_anchor() {
    let _permit = crate::support::test_subprocess::acquire();
    let repo = init_repo();
    let worktree_root = tempfile::tempdir().unwrap();
    let worktree = worktree_root.path().join("linked");
    git(
        repo.path(),
        &["worktree", "add", "-b", "aid/retry-legacy", &worktree.to_string_lossy()],
    );
    let mut task = failed_task("t-legacy-live-worktree");
    task.worktree_path = Some(worktree.to_string_lossy().to_string());
    task.worktree_branch = Some("aid/retry-legacy".to_string());
    let mut args = RunArgs {
        dir: Some(worktree.to_string_lossy().to_string()),
        ..Default::default()
    };

    apply_retry_target(&task, &mut args).unwrap();

    // The repo anchor is derived via git, which reports the canonical path. On macOS the
    // tempdir lives under /var, a symlink to /private/var, so compare canonicalized forms.
    let expected_repo = repo.path().canonicalize().expect("canonicalize repo path");
    let actual_repo = std::path::Path::new(args.repo.as_deref().expect("repo anchor"))
        .canonicalize()
        .expect("canonicalize derived repo anchor");
    assert_eq!(actual_repo, expected_repo);
    assert_eq!(args.dir.as_deref(), Some(worktree.to_str().unwrap()));
    assert_eq!(args.worktree.as_deref(), Some("aid/retry-legacy"));
}
