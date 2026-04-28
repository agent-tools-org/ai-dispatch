// Worktree path tests for the persisted ~/.aid/worktrees layout.
// Exports: none.
// Deps: super helpers, tempfile, std::process::Command.

use super::{
    aid_worktree_path, aid_worktree_root, create_worktree,
    is_aid_managed_worktree_path,
};
use super::path::WorktreeHomeGuard;
use crate::test_subprocess;
use std::path::Path;
use std::process::Command;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo(repo_dir: &Path) {
    git(repo_dir, &["init", "-b", "main"]);
    git(repo_dir, &["config", "user.email", "test@example.com"]);
    git(repo_dir, &["config", "user.name", "Test User"]);
    std::fs::write(repo_dir.join("file.txt"), "hello\n").unwrap();
    git(repo_dir, &["add", "file.txt"]);
    git(repo_dir, &["commit", "-m", "init"]);
}

#[test]
fn create_worktree_uses_aid_home_project_branch_path() {
    let permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let home_guard = WorktreeHomeGuard::set(home.path());
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());

    let branch = "feat/home-root";
    let info = create_worktree(repo.path(), branch, None).unwrap();
    let project = repo.path().canonicalize().unwrap();
    let project = project.file_name().unwrap().to_string_lossy();
    let expected = home
        .path()
        .join(".aid")
        .join("worktrees")
        .join(project.as_ref())
        .join(branch);

    assert_eq!(aid_worktree_root(), home.path().join(".aid").join("worktrees"));
    assert_eq!(aid_worktree_path(repo.path(), branch), expected);
    assert_eq!(info.path, expected);
    assert!(info.path.exists());
    assert!(info.path.parent().unwrap().exists());
    assert!(is_aid_managed_worktree_path(&info.path));

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &info.path.to_string_lossy(),
        ],
    );
    drop(home_guard);
    drop(permit);
}
