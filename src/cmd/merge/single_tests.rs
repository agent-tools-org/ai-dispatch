// Tests for merge single behavior.
// Uses shared merge fixtures and git helpers from the parent test module.

use super::*;

#[test]
fn tracked_merge_backup_survives_aggressive_git_gc() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();

    let changes = stash_local_changes(&repo.path().to_string_lossy())
        .unwrap()
        .expect("local changes should be captured");
    git(repo.path(), &["gc", "--prune=now"]);
    restore_local_changes(&repo.path().to_string_lossy(), &changes).unwrap();

    assert_eq!(
        std::fs::read_to_string(repo.path().join("init.txt")).unwrap(),
        "local change\n"
    );
}

#[test]
fn resolve_repo_dir_prefers_explicit_repo_path() {
    let _permit = test_subprocess::acquire();
    let result = resolve_repo_dir(Some("/explicit/repo"), Some("/tmp/worktree"));
    assert_eq!(result, "/explicit/repo");
}

#[test]
fn resolve_repo_dir_detects_from_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, _branch) = create_worktree_with_commit(repo.path());

    let result = resolve_repo_dir(None, Some(&wt.path().to_string_lossy()));
    // Should resolve to the main repo, not the worktree
    let canon_repo = repo.path().canonicalize().unwrap();
    let canon_result = Path::new(&result).canonicalize().unwrap();
    assert_eq!(canon_result, canon_repo);

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &wt.path().to_string_lossy(),
        ],
    );
}

#[test]
fn resolve_repo_dir_falls_back_to_dot() {
    let _permit = test_subprocess::acquire();
    let result = resolve_repo_dir(None, None);
    assert_eq!(result, ".");
}

#[test]
fn sync_cargo_lock_before_merge_commits_updated_lockfile() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("Cargo.lock"), "version = 1\n").unwrap();
    git(repo.path(), &["add", "Cargo.lock"]);
    git(repo.path(), &["commit", "-m", "add lockfile"]);

    let (wt, branch) = create_empty_worktree_branch(repo.path());
    std::fs::write(repo.path().join("Cargo.lock"), "version = 2\n").unwrap();
    git(repo.path(), &["add", "Cargo.lock"]);
    git(repo.path(), &["commit", "-m", "update lockfile"]);

    sync_cargo_lock_before_merge(
        &repo.path().to_string_lossy(),
        &wt.path().to_string_lossy(),
        &branch,
    );

    assert_eq!(
        std::fs::read_to_string(wt.path().join("Cargo.lock")).unwrap(),
        "version = 2\n"
    );
    assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);

    let log = Command::new("git")
        .args([
            "-C",
            &wt.path().to_string_lossy(),
            "log",
            "-1",
            "--pretty=%s",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "chore: sync Cargo.lock from main"
    );

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &wt.path().to_string_lossy(),
        ],
    );
}

// --- Integration tests for merge_single ---

#[test]
fn merge_single_succeeds_with_committed_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-merge-ok", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-merge-ok", false, false, false, None);
    assert!(result.is_ok(), "merge_single failed: {result:?}");

    let loaded = store.get_task("t-merge-ok").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
    assert!(repo.path().join("agent-work.txt").exists());
}

#[test]
fn merge_single_refuses_required_verification_without_force() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-merge-unverified", repo.path(), wt.path(), &branch);
    task.verify = Some("cargo test".to_string());
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, task.id.as_str(), false, false, false, None);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--force"));
}

#[test]
fn merge_single_auto_commits_then_merges() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("uncommitted");
    let wt = TempDir::new().unwrap();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            &branch,
        ],
    );
    // Leave changes uncommitted — this is the data-loss scenario
    std::fs::write(
        wt.path().join("uncommitted.txt"),
        "agent forgot to commit\n",
    )
    .unwrap();

    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-autocommit", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-autocommit", false, false, false, None);
    assert!(
        result.is_ok(),
        "merge_single should auto-commit and merge: {result:?}"
    );

    let loaded = store.get_task("t-autocommit").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
    assert!(repo.path().join("uncommitted.txt").exists());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("uncommitted.txt")).unwrap(),
        "agent forgot to commit\n"
    );
}

#[test]
fn merge_group_refuses_failed_verification_without_force() {
    let store = Store::open_memory().unwrap();
    let group = "wg-merge-vfail";
    let mut task =
        make_task_with_worktree("t-group-vfail", Path::new("."), Path::new("/tmp"), "branch");
    task.worktree_path = None;
    task.worktree_branch = None;
    task.repo_path = None;
    task.workgroup_id = Some(group.to_string());
    task.verify_status = VerifyStatus::Failed;
    store.insert_task(&task).unwrap();

    let result = merge_group_with_output(&store, group, false, false, false, None, false);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--force"));
}

#[test]
fn merge_group_skips_an_already_merged_task() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    let mut task =
        make_task_with_worktree("t-group-already-merged", repo.path(), wt.path(), &branch);
    task.status = TaskStatus::Merged;
    task.workgroup_id = Some("wg-already-merged".to_string());
    store.insert_task(&task).unwrap();

    let result = merge_group_with_output(
        &store,
        "wg-already-merged",
        false,
        false,
        false,
        None,
        false,
    );

    assert!(
        result.is_ok(),
        "already-merged task should be skipped: {result:?}"
    );
    assert!(!repo.path().join("agent-work.txt").exists());
}

#[test]
fn merge_single_fails_when_no_commits_and_no_changes() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("empty");
    let wt = TempDir::new().unwrap();
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            &branch,
        ],
    );
    // No changes at all — nothing to merge

    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-empty", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-empty", false, false, false, None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No commits to merge"),
        "unexpected error: {err}"
    );

    // Task should still be Done (not Merged)
    let loaded = store.get_task("t-empty").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);
    // Worktree should be preserved
    assert!(wt.path().exists());

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &wt.path().to_string_lossy(),
        ],
    );
}
