// Tests for merge staging exclusions and completed-task guards.
// Uses shared merge fixtures from the parent test module.

use super::*;

#[test]
fn auto_commit_skips_aid_lock_only_changes() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("aid-lock-only");
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
    // Write .aid-lock as the only change
    std::fs::write(wt.path().join(".aid-lock"), "pid=1234\n").unwrap();

    let before = Command::new("git")
        .args(["-C", &wt.path().to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    let before_sha = String::from_utf8_lossy(&before.stdout).trim().to_string();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(!committed, "should not commit when only .aid-lock changed");

    let after = Command::new("git")
        .args(["-C", &wt.path().to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    let after_sha = String::from_utf8_lossy(&after.stdout).trim().to_string();
    assert_eq!(
        before_sha, after_sha,
        "HEAD should not change when only .aid-lock is dirty"
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
fn auto_commit_excludes_aid_lock_when_other_files_change() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("aid-lock-and-source");
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
    // Write both .aid-lock and a real source file
    std::fs::write(wt.path().join(".aid-lock"), "pid=9999\n").unwrap();

    let before = Command::new("git")
        .args(["-C", &wt.path().to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    let before_sha = String::from_utf8_lossy(&before.stdout).trim().to_string();

    // Create a real source file change
    std::fs::create_dir_all(wt.path().join("src")).unwrap();
    std::fs::write(wt.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(committed, "should commit when real source files changed");

    let after = Command::new("git")
        .args(["-C", &wt.path().to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    let after_sha = String::from_utf8_lossy(&after.stdout).trim().to_string();
    assert_ne!(
        before_sha, after_sha,
        "HEAD should advance when source files changed"
    );

    // .aid-lock must NOT be in the commit
    let show = Command::new("git")
        .args([
            "-C",
            &wt.path().to_string_lossy(),
            "show",
            "--stat",
            "--format=",
            "HEAD",
        ])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&show.stdout);
    assert!(
        !stat.contains(".aid-lock"),
        ".aid-lock should not be in the commit, but was: {stat}"
    );
    assert!(
        stat.contains("src/main.rs"),
        "src/main.rs should be in the commit"
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
fn merge_single_refuses_an_already_merged_task() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-already-merged", repo.path(), wt.path(), &branch);
    task.status = TaskStatus::Merged;
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, task.id.as_str(), false, false, false, None, true);

    assert!(
        result.is_err(),
        "merging an already-merged task must not proceed"
    );
}
