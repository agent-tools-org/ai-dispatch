// Tests for merge merge check behavior.
// Uses shared merge fixtures and git helpers from the parent test module.

use super::*;

#[test]
fn git_merge_branch_merges_committed_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Merged));
    // Verify the file landed in main
    assert!(repo.path().join("agent-work.txt").exists());

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
fn git_merge_branch_detects_already_up_to_date() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("noop-branch");
    git(repo.path(), &["branch", &branch]);

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::AlreadyUpToDate));
}

#[test]
fn checkout_branch_switches_head() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("target");
    git(repo.path(), &["branch", &branch]);

    checkout_branch(&repo.path().to_string_lossy(), &branch).unwrap();

    let output = Command::new("git")
        .args([
            "-C",
            &repo.path().to_string_lossy(),
            "branch",
            "--show-current",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), branch);
}

#[test]
fn git_merge_branch_detects_conflict() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("conflict-branch");
    let wt = create_conflict_worktree(repo.path(), &branch);

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Failed(_)));
    // Abort the failed merge
    let _ = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "merge", "--abort"])
        .output();
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
fn check_merge_detects_clean_merge() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());

    let result = check_merge(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeCheckResult::Ok(1)));
    assert_eq!(worktree_status(repo.path()), "");
    assert!(!repo.path().join("agent-work.txt").exists());

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
fn check_merge_detects_conflict() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("check-conflict");
    let wt = create_conflict_worktree(repo.path(), &branch);

    let result = check_merge(&repo.path().to_string_lossy(), &branch);
    match result {
        MergeCheckResult::Conflict(files) => assert_eq!(files, vec!["init.txt".to_string()]),
        MergeCheckResult::Ok(commits) => {
            panic!("expected conflict, got clean merge with {commits} commit(s)")
        }
        MergeCheckResult::StashRestoreFailed(error) => {
            panic!("expected merge conflict, got stash restore failure: {error}")
        }
    }
    assert_eq!(worktree_status(repo.path()), "");

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
fn git_merge_branch_stashes_local_changes() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let local_file = repo.path().join("init.txt");
    std::fs::write(&local_file, "local change\n").unwrap();
    let (wt, branch) = create_worktree_with_commit(repo.path());

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Merged));
    assert_eq!(
        std::fs::read_to_string(local_file).unwrap(),
        "local change\n"
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
