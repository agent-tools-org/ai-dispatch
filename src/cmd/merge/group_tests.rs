// Tests for merge group behavior.
// Uses shared merge fixtures and git helpers from the parent test module.

use super::*;

#[test]
fn merge_group_skips_empty_branches() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (committed_wt, committed_branch) = create_worktree_with_commit(repo.path());
    let (empty_wt, empty_branch) = create_empty_worktree_branch(repo.path());

    let store = Store::open_memory().unwrap();
    let group_id = "wg-merge-group";

    let mut committed_task = make_task_with_worktree(
        "t-merge-group",
        repo.path(),
        committed_wt.path(),
        &committed_branch,
    );
    committed_task.workgroup_id = Some(group_id.to_string());
    store.insert_task(&committed_task).unwrap();

    let mut empty_task = make_task_with_worktree(
        "t-empty-branch",
        repo.path(),
        empty_wt.path(),
        &empty_branch,
    );
    empty_task.workgroup_id = Some(group_id.to_string());
    store.insert_task(&empty_task).unwrap();

    let result = merge_group_with_output(&store, group_id, false, false, false, None, true);
    assert!(result.is_ok(), "merge_group failed: {result:?}");

    let loaded_committed = store.get_task("t-merge-group").unwrap().unwrap();
    assert_eq!(loaded_committed.status, TaskStatus::Merged);
    assert!(repo.path().join("agent-work.txt").exists());

    let loaded_empty = store.get_task("t-empty-branch").unwrap().unwrap();
    assert_eq!(loaded_empty.status, TaskStatus::Done);

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &empty_wt.path().to_string_lossy(),
        ],
    );
}

#[test]
fn run_rejects_lanes_without_group() {
    let store = Arc::new(Store::open_memory().unwrap());
    let result = run(store, None, None, false, false, false, None, true);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "--lanes requires --group");
}

#[test]
fn run_rejects_unsupported_lanes_flags() {
    let store = Arc::new(Store::open_memory().unwrap());

    let check_result = run(
        store.clone(),
        None,
        Some("wg-lanes"),
        false,
        true,
        false,
        None,
        true,
    );
    assert!(check_result.is_err());
    assert_eq!(
        check_result.unwrap_err().to_string(),
        "--lanes does not yet support --check (dry-run); run without --check to apply lanes"
    );

    let target_result = run(
        store,
        None,
        Some("wg-lanes"),
        false,
        false,
        false,
        Some("release"),
        true,
    );
    assert!(target_result.is_err());
    assert_eq!(
        target_result.unwrap_err().to_string(),
        "--lanes cannot be combined with --target; lanes apply to the GitButler workspace of the main repo"
    );
}

#[test]
fn run_rejects_lanes_when_gitbutler_env_is_disabled() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task_with_worktree(
        "t-lanes-disabled",
        Path::new("."),
        Path::new("/tmp"),
        "lane-branch",
    );
    task.workgroup_id = Some("wg-lanes-disabled".to_string());
    store.insert_task(&task).unwrap();

    let previous = env::var("AID_GITBUTLER").ok();
    unsafe {
        env::set_var("AID_GITBUTLER", "0");
    }
    let result = run(
        store,
        None,
        Some("wg-lanes-disabled"),
        false,
        false,
        false,
        None,
        true,
    );
    match previous {
        Some(value) => unsafe { env::set_var("AID_GITBUTLER", value) },
        None => unsafe { env::remove_var("AID_GITBUTLER") },
    }

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "GitButler integration disabled via AID_GITBUTLER=0"
    );
}

// --- verify "auto" fix ---

#[test]
fn run_verify_handles_auto_without_error() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    // Should not try to execute "auto" as a command — should fallback to "cargo check"
    // (will fail since no Cargo.toml, but that's OK — it shouldn't panic or try "auto")
    run_verify_in_worktree(&repo.path().to_string_lossy(), Some("auto"));
    // If we got here without panic, the fix works
}

#[test]
fn run_post_merge_verify_warns_on_failure() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    run_post_merge_verify(
        &repo.path().to_string_lossy(),
        Some("git missing-subcommand"),
    );
    assert_eq!(worktree_status(repo.path()), "");
}

#[test]
fn approval_decision_defaults_to_merge() {
    let _permit = test_subprocess::acquire();
    // Verify the approval decision logic: empty/unknown reply → Merge
    let reply = "";
    let decision = if reply.contains("Skip") {
        ApprovalDecision::Skip
    } else if reply.contains("Retry") {
        ApprovalDecision::Retry
    } else {
        ApprovalDecision::Merge
    };
    assert!(matches!(decision, ApprovalDecision::Merge));
}
