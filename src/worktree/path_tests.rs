// Worktree path tests for the persisted ~/.aid/worktrees layout.
// Exports: none.
// Deps: super helpers, tempfile, std::process::Command.

use super::{
    aid_worktree_path, aid_worktree_root, create_worktree,
    is_aid_managed_worktree_path, is_safe_worktree_path, remove_worktree,
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

fn unique_branch(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[test]
fn remove_worktree_cleans_up_properly() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let branch = unique_branch("cleanup");
    // Use /tmp/aid-wt-* path to pass sandbox guard
    let wt_path = format!("/tmp/aid-wt-test-{branch}");
    git(repo.path(), &["worktree", "add", &wt_path, "-b", &branch]);

    // Should not panic and worktree dir should be gone
    remove_worktree(&repo.path().to_string_lossy(), &wt_path).unwrap();
    assert!(!Path::new(&wt_path).exists());

    // git worktree list should not show it
    let out = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "worktree", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(&branch));
}

#[test]
fn sandbox_allows_aid_worktree_paths() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _home_guard = WorktreeHomeGuard::set(home.path());
    let home_path = aid_worktree_root().join("demo").join("feat/foo");
    assert!(is_safe_worktree_path(&home_path.to_string_lossy()));
    assert!(is_safe_worktree_path("/tmp/aid-wt-feat-foo"));
    assert!(is_safe_worktree_path("/tmp/aid-wt-fix/bar"));
    assert!(is_safe_worktree_path("/private/tmp/aid-wt-test"));
}

#[test]
fn sandbox_blocks_non_worktree_paths() {
    let _permit = test_subprocess::acquire();
    assert!(!is_safe_worktree_path("/home/user/project"));
    assert!(!is_safe_worktree_path("/Users/someone/Develop/myrepo"));
    assert!(!is_safe_worktree_path("/tmp/other-dir"));
    assert!(!is_safe_worktree_path("/tmp/aid-wt")); // missing trailing dash
    assert!(!is_safe_worktree_path("/tmp"));
    assert!(!is_safe_worktree_path(""));
    assert!(!is_safe_worktree_path("/"));
}

#[test]
fn remove_worktree_refuses_unsafe_path() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let unsafe_path = repo.path().join("subdir");
    std::fs::create_dir_all(&unsafe_path).unwrap();

    // This should NOT delete the directory — sandbox guard blocks it
    let result = remove_worktree(
        &repo.path().to_string_lossy(),
        &unsafe_path.to_string_lossy(),
    );
    assert!(result.is_err());
    // Directory must still exist
    assert!(
        unsafe_path.exists(),
        "Sandbox guard failed: unsafe path was deleted!"
    );
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
