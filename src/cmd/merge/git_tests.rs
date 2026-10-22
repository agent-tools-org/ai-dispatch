// Tests for merge git behavior.
// Uses shared merge fixtures and git helpers from the parent test module.

use super::*;

#[test]
fn commits_ahead_detects_branch_with_commits() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);
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
fn commits_ahead_returns_zero_for_same_head() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("empty-branch");
    git(repo.path(), &["branch", &branch]);
    assert_eq!(commits_ahead(&repo.path().to_string_lossy(), &branch), 0);
}

#[test]
fn commits_ahead_returns_zero_for_missing_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    assert_eq!(
        commits_ahead(&repo.path().to_string_lossy(), "nonexistent"),
        0
    );
}

#[test]
fn auto_commit_uncommitted_commits_dirty_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("dirty-branch");
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
    // Leave changes uncommitted
    std::fs::write(wt.path().join("dirty.txt"), "uncommitted\n").unwrap();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(committed);
    // Now the branch should have commits ahead
    assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);

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
fn auto_commit_uncommitted_excludes_aid_directory() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("aid-dir-branch");
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
    std::fs::write(wt.path().join("dirty.txt"), "uncommitted\n").unwrap();
    std::fs::create_dir_all(wt.path().join(".aid")).unwrap();
    std::fs::write(wt.path().join(".aid/state.toml"), "health = 1\n").unwrap();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(committed);

    let out = Command::new("git")
        .args([
            "-C",
            &wt.path().to_string_lossy(),
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "HEAD",
        ])
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&out.stdout);
    assert!(
        files.lines().any(|line| line == "dirty.txt"),
        "got: {files}"
    );
    assert!(
        !files.lines().any(|line| line == ".aid/state.toml"),
        "got: {files}"
    );
    assert!(wt.path().join(".aid/state.toml").exists());

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
fn merge_refuses_persisted_worktree_that_equals_repo_path() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("dirty.txt"), "do not commit\n").unwrap();
    let task = make_task_with_worktree("t-poisoned-merge", repo.path(), repo.path(), "main");
    let store = Store::open_memory().unwrap();
    store.insert_task(&task).unwrap();

    let err = merge_single_with_output(&store, task.id.as_str(), false, false, false, None, false)
        .unwrap_err();

    assert!(err.to_string().contains("recorded worktree path"));
    assert_eq!(head_message(repo.path()), "init");
    assert!(!worktree_status(repo.path()).is_empty());
}

#[test]
fn auto_commit_message_includes_filename() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("message-branch");
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
    std::fs::write(wt.path().join("named-file.txt"), "uncommitted\n").unwrap();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(committed);

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
        "chore: auto-commit changes to named-file.txt"
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

#[test]
fn auto_commit_uncommitted_returns_false_for_clean_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(!committed);
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
