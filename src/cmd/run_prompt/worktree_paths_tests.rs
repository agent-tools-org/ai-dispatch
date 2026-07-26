// Tests for run prompt worktree path resolution.
// Exports: none.
// Deps: run_prompt helpers, git subprocess fixtures, tempfile.

use super::*;
use crate::test_subprocess;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success());
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_git_repo_with_commit(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    std::fs::write(dir.join("file.txt"), "hello\n").unwrap();
    git(dir, &["add", "file.txt"]);
    git(dir, &["commit", "-m", "init"]);
}

#[test]
fn resolve_worktree_paths_recreates_pruned_existing_branch_at_tip() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_git_repo_with_commit(repo.path());
    let branch = format!("feat/run-resume-{}", std::process::id());
    let first = crate::worktree::create_worktree(repo.path(), &branch, Some("main")).unwrap();
    std::fs::write(first.path.join("agent.txt"), "agent\n").unwrap();
    git(&first.path, &["add", "agent.txt"]);
    git(&first.path, &["commit", "-m", "agent commit"]);
    let branch_head = git_output(repo.path(), &["rev-parse", &branch]);
    std::fs::remove_dir_all(&first.path).unwrap();

    let paths = resolve_worktree_paths(
        &RunArgs {
            dir: Some(repo.path().to_string_lossy().to_string()),
            worktree: Some(branch.clone()),
            ..Default::default()
        },
        Some(repo.path().to_str().unwrap()),
    )
    .unwrap();

    let worktree_path = Path::new(paths.0.as_deref().expect("worktree path"));
    assert_eq!(git_output(repo.path(), &["rev-parse", &branch]), branch_head);
    assert_eq!(std::fs::read_to_string(worktree_path.join("agent.txt")).unwrap(), "agent\n");
}

#[test]
fn resolve_worktree_paths_uses_explicit_repo_before_dir_fallback() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_git_repo_with_commit(repo.path());
    let bad_dir = repo.path().join("missing").join("child");

    let paths = resolve_worktree_paths(
        &RunArgs {
            dir: Some(bad_dir.to_string_lossy().to_string()),
            worktree: Some("chore/repro-eager".to_string()),
            ..Default::default()
        },
        Some(repo.path().to_str().unwrap()),
    )
    .unwrap();

    assert_eq!(paths.3.as_deref(), Some(repo.path().to_str().unwrap()));
    assert!(paths.0.as_deref().is_some_and(|path| Path::new(path).exists()));
}

#[test]
fn resolve_worktree_paths_without_repo_still_rejects_bad_dir() {
    let bad_dir = tempfile::tempdir().unwrap().path().join("missing");

    let err = resolve_worktree_paths(
        &RunArgs {
            dir: Some(bad_dir.to_string_lossy().to_string()),
            worktree: Some("chore/repro-bad-dir".to_string()),
            ..Default::default()
        },
        None,
    )
    .unwrap_err();

    assert!(err.to_string().contains("Not a git repository"));
}
