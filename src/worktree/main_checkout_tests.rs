// Worktree safety tests for main-checkout branch reuse.
// Exports: none.
// Deps: create_worktree, existing_worktree_path, real git temp repos.

use super::*;
use crate::test_subprocess;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

#[test]
fn existing_worktree_path_ignores_main_checkout_and_finds_linked_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["checkout", "-b", "chore/main-only"]);

    let main_only = existing_worktree_path(repo.path(), "chore/main-only").unwrap();
    assert_eq!(main_only, None);

    git(repo.path(), &["checkout", "main"]);
    let linked_root = TempDir::new().unwrap();
    let linked_path = linked_root.path().join("linked");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "chore/linked",
            &linked_path.to_string_lossy(),
        ],
    );

    let found = existing_worktree_path(repo.path(), "chore/linked")
        .unwrap()
        .unwrap();
    assert_eq!(canonical_worktree_path(&found), canonical_worktree_path(&linked_path));

    git(repo.path(), &["worktree", "remove", "--force", &linked_path.to_string_lossy()]);
}

#[test]
fn create_worktree_rejects_branch_checked_out_in_main_checkout() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["checkout", "-b", "chore/main-checked-out"]);

    let err = create_worktree(repo.path(), "chore/main-checked-out", None).unwrap_err();

    assert!(err.to_string().contains("main working tree"));
    assert!(!err.to_string().contains(repo.path().to_string_lossy().as_ref()));
}

#[test]
fn create_worktree_rejects_branch_checked_out_in_caller_linked_checkout() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let linked_root = TempDir::new().unwrap();
    let linked_path = linked_root.path().join("linked");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "chore/linked-dispatch",
            &linked_path.to_string_lossy(),
        ],
    );

    let err = create_worktree(&linked_path, "chore/linked-dispatch", None).unwrap_err();

    assert!(err.to_string().contains("caller checkout"));
    git(repo.path(), &["worktree", "remove", "--force", &linked_path.to_string_lossy()]);
}

#[test]
fn create_worktree_from_linked_checkout_allows_different_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let linked_root = TempDir::new().unwrap();
    let linked_path = linked_root.path().join("linked");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "chore/source-linked",
            &linked_path.to_string_lossy(),
        ],
    );

    let info = create_worktree(&linked_path, "chore/different-linked-target", None).unwrap();

    assert_ne!(canonical_worktree_path(&info.path), canonical_worktree_path(&linked_path));
    assert_ne!(canonical_worktree_path(&info.path), canonical_worktree_path(repo.path()));
    git(repo.path(), &["worktree", "remove", "--force", &info.path.to_string_lossy()]);
    git(repo.path(), &["worktree", "remove", "--force", &linked_path.to_string_lossy()]);
}

#[test]
fn create_worktree_rejects_branch_checked_out_in_submodule_main_checkout() {
    let _permit = test_subprocess::acquire();
    let outer = init_repo();
    let sub_source = init_repo();
    let outer_path = outer.path().canonicalize().unwrap();
    let sub_source_path = sub_source.path().canonicalize().unwrap();
    let sub_path = outer_path.join("deps").join("sub");
    assert!(Command::new("git")
        .args(["-C", &outer_path.to_string_lossy()])
        .args(["-c", "protocol.file.allow=always"])
        .args(["submodule", "add", &sub_source_path.to_string_lossy(), &sub_path.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    git(&sub_path, &["checkout", "-b", "chore/submodule-main"]);

    let err = create_worktree(&sub_path, "chore/submodule-main", None).unwrap_err();
    let message = err.to_string();

    assert!(
        message.contains("main working tree")
            || message.contains("caller checkout")
            || message.contains("cannot determine main checkout"),
        "{message}"
    );
}

#[test]
fn create_worktree_allows_normal_fresh_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();

    let info = create_worktree(repo.path(), "chore/fresh-normal", None).unwrap();

    assert!(info.created);
    assert_ne!(canonical_worktree_path(&info.path), canonical_worktree_path(repo.path()));
    git(repo.path(), &["worktree", "remove", "--force", &info.path.to_string_lossy()]);
}
