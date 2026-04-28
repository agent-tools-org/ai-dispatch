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

fn expected_project_id(repo_dir: &Path) -> String {
    let canonical = repo_dir.canonicalize().unwrap();
    let basename = canonical.file_name().unwrap().to_string_lossy();
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        canonical.to_string_lossy().hash(&mut hasher);
        format!("{:x}", hasher.finish())
    };
    format!("{basename}-{}", hash.chars().take(8).collect::<String>())
}

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
    let expected = home
        .path()
        .join(".aid")
        .join("worktrees")
        .join(expected_project_id(repo.path()))
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

#[test]
fn managed_path_rejects_relative_and_traversal_paths() {
    let home = tempfile::tempdir().unwrap();
    let home_guard = WorktreeHomeGuard::set(home.path());
    let traversal = aid_worktree_root()
        .join("project")
        .join("..")
        .join("..")
        .join("outside");

    assert!(!is_aid_managed_worktree_path(Path::new("relative/path")));
    assert!(!is_aid_managed_worktree_path(&traversal));

    drop(home_guard);
}

#[test]
fn managed_path_accepts_nonexistent_child_under_aid_root() {
    let home = tempfile::tempdir().unwrap();
    let home_guard = WorktreeHomeGuard::set(home.path());
    let path = aid_worktree_root()
        .join("project")
        .join("feat")
        .join("nonexistent");

    assert!(!path.exists());
    assert!(is_aid_managed_worktree_path(&path));

    drop(home_guard);
}

#[test]
fn path_from_linked_worktree_uses_main_repo_project_id() {
    let permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let home_guard = WorktreeHomeGuard::set(home.path());
    let repo = tempfile::tempdir().unwrap();
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("linked-worktree");
    init_repo(repo.path());

    git(
        repo.path(),
        &["worktree", "add", &linked.to_string_lossy(), "-b", "feat/linked"],
    );

    let branch = "feat/from-linked";
    let from_main = aid_worktree_path(repo.path(), branch);
    let from_linked = aid_worktree_path(&linked, branch);
    let linked_name = linked.file_name().unwrap().to_string_lossy();

    assert_eq!(from_linked, from_main);
    assert_eq!(
        from_linked,
        home.path()
            .join(".aid")
            .join("worktrees")
            .join(expected_project_id(repo.path()))
            .join(branch)
    );
    assert!(!from_linked.to_string_lossy().contains(linked_name.as_ref()));

    git(
        repo.path(),
        &["worktree", "remove", "--force", &linked.to_string_lossy()],
    );
    drop(home_guard);
    drop(permit);
}
