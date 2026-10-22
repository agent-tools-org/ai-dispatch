// Tests for merge stash behavior.
// Uses shared merge fixtures and git helpers from the parent test module.

use super::*;

#[test]
fn stash_capture_keeps_identity_when_a_competing_stash_appears() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();
    let changes = stash_local_changes_with_hook(&repo.path().to_string_lossy(), || {
        std::fs::write(repo.path().join("init.txt"), "competing\n").unwrap();
        git(
            repo.path(),
            &["stash", "push", "-q", "-m", "competing stash"],
        );
    })
    .unwrap()
    .expect("local changes should be captured");
    restore_local_changes(&repo.path().to_string_lossy(), &changes).unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.path().join("init.txt")).unwrap(),
        "local change\n"
    );
    let stashes = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "stash", "list"])
        .output()
        .unwrap();
    let stashes = String::from_utf8_lossy(&stashes.stdout);
    assert!(stashes.contains("competing stash"));
    assert!(stashes.contains("aid merge-local"));
    git(repo.path(), &["stash", "drop"]);
}

#[test]
fn stash_capture_fails_when_git_status_cannot_run() {
    let _permit = test_subprocess::acquire();
    let result = stash_local_changes("/path/that/is/not/a/git/repository");
    assert!(matches!(result, Err(error) if error.contains("status")));
}

#[test]
fn stash_capture_reports_token_on_post_push_identity_failure() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();
    let result =
        stash_local_changes_with_identity_hook(&repo.path().to_string_lossy(), |message| {
            std::fs::write(repo.path().join("foreign.txt"), "foreign\n").unwrap();
            git(
                repo.path(),
                &[
                    "stash",
                    "push",
                    "--include-untracked",
                    "--quiet",
                    "--message",
                    message,
                ],
            );
        });
    assert!(matches!(result, Err(error)
        if error.contains("multiple stashes matched")
            && error.contains("recovery handle: stash message aid merge-local")));
    git(repo.path(), &["stash", "clear"]);
}

#[test]
fn stash_identity_lookup_uses_token_when_banner_text_is_unusable() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let token = format!("aid merge-local {}", unique("token"));
    let message = format!("translated banner text {token}");
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();
    git(
        repo.path(),
        &[
            "stash",
            "push",
            "--include-untracked",
            "--quiet",
            "--message",
            &message,
        ],
    );
    let found = find_stash_for_test(&repo.path().to_string_lossy(), &token).unwrap();
    let expected = Command::new("git")
        .args([
            "-C",
            &repo.path().to_string_lossy(),
            "rev-parse",
            "refs/stash",
        ])
        .output()
        .unwrap();
    assert_eq!(found, String::from_utf8_lossy(&expected.stdout).trim());
    git(repo.path(), &["stash", "clear"]);
}

#[test]
fn stash_capture_does_not_reset_edits_written_after_capture() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();
    let changes = stash_local_changes_with_hook(&repo.path().to_string_lossy(), || {
        std::fs::write(repo.path().join("late.txt"), "late change\n").unwrap();
    });
    let changes = changes.unwrap().expect("local changes should be captured");
    assert_eq!(
        std::fs::read_to_string(repo.path().join("late.txt")).unwrap(),
        "late change\n"
    );
    restore_local_changes(&repo.path().to_string_lossy(), &changes).unwrap();
}

#[test]
fn detached_head_capture_and_restore_keeps_local_changes() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["checkout", "--detach", "HEAD"]);
    std::fs::write(repo.path().join("init.txt"), "detached local change\n").unwrap();
    let changes = stash_local_changes(&repo.path().to_string_lossy())
        .unwrap()
        .expect("local changes should be captured");
    restore_local_changes(&repo.path().to_string_lossy(), &changes).unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.path().join("init.txt")).unwrap(),
        "detached local change\n"
    );
}

#[test]
fn untracked_restore_reports_every_colliding_file_and_backup_path() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::write(repo.path().join("first.txt"), "user first\n").unwrap();
    std::fs::write(repo.path().join("second.txt"), "user second\n").unwrap();
    let changes = stash_local_changes(&repo.path().to_string_lossy())
        .unwrap()
        .expect("untracked changes should be captured");
    std::fs::write(repo.path().join("first.txt"), "merge first\n").unwrap();
    std::fs::write(repo.path().join("second.txt"), "merge second\n").unwrap();
    let result = restore_local_changes(&repo.path().to_string_lossy(), &changes);
    assert!(
        matches!(&result, Err(error)
        if error.contains("first.txt")
            && error.contains("second.txt")
            && error.contains("stash commit")),
        "restore result: {result:?}"
    );
}

#[test]
fn git_merge_branch_stashes_untracked_changes_without_using_an_older_stash() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    std::fs::write(repo.path().join("init.txt"), "older rescue\n").unwrap();
    git(repo.path(), &["stash", "push", "-m", "older rescue"]);
    std::fs::write(repo.path().join("task-b-untracked.txt"), "task-b\n").unwrap();

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Merged));
    assert_eq!(
        std::fs::read_to_string(repo.path().join("init.txt")).unwrap(),
        "init\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("task-b-untracked.txt")).unwrap(),
        "task-b\n"
    );
    let stashes = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "stash", "list"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&stashes.stdout).contains("older rescue"));
    git(repo.path(), &["stash", "drop"]);
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
fn git_merge_branch_restores_its_own_stash_when_newer_stash_appears() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    std::fs::write(repo.path().join("main.txt"), "main\n").unwrap();
    git(repo.path(), &["add", "main.txt"]);
    git(repo.path(), &["commit", "-m", "main change"]);
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();

    let hook = repo.path().join(".git/hooks/pre-merge-commit");
    crate::support::test_env::write_executable(
        &hook,
        "#!/bin/sh\nprintf 'task-b rescue\n' > init.txt\ngit stash push -q -m 'task-b rescue'\n",
    );

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Merged));
    assert_eq!(
        std::fs::read_to_string(repo.path().join("init.txt")).unwrap(),
        "local change\n"
    );
    let stashes = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "stash", "list"])
        .output()
        .unwrap();
    let stashes = String::from_utf8_lossy(&stashes.stdout);
    assert!(stashes.contains("task-b rescue"));
    assert!(!stashes.contains("aid: auto-stash before merge"));
    git(repo.path(), &["stash", "drop"]);
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
fn git_merge_branch_fails_loudly_on_stash_restore_conflict() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("pop-conflict");
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
    std::fs::write(wt.path().join("init.txt"), "branch change\n").unwrap();
    git(wt.path(), &["add", "init.txt"]);
    git(wt.path(), &["commit", "-m", "branch change"]);
    std::fs::write(repo.path().join("init.txt"), "main change\n").unwrap();
    git(repo.path(), &["add", "init.txt"]);
    git(repo.path(), &["commit", "-m", "main change"]);
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();
    std::fs::write(repo.path().join("untracked.txt"), "keep me\n").unwrap();

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    let MergeResult::Failed(error) = result else {
        panic!("expected merge failure");
    };
    assert!(
        error.contains("tracked changes were kept out of the conflicted index"),
        "merge error: {error}"
    );
    assert!(
        error.contains("stash commit"),
        "recovery details missing: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("untracked.txt")).unwrap(),
        "keep me\n"
    );
    let status = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "status", "--short"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("UU init.txt"));

    git(repo.path(), &["reset", "--hard", "HEAD~1"]);
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
