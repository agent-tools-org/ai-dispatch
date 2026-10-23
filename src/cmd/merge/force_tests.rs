// Tests for merge force behavior.
// Uses shared merge fixtures and git helpers from the parent test module.

use super::*;

#[test]
fn merge_single_rejects_non_done_task() {
    let _permit = test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-running", Path::new("."), Path::new("/tmp"), "b");
    task.status = TaskStatus::Running;
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-running", false, false, false, None, true);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("only DONE"));
}

#[test]
fn merge_single_rejects_failed_task_without_force() {
    let _permit = test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-failed", Path::new("."), Path::new("/tmp"), "b");
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-failed", false, false, false, None, true);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("only DONE"));
}

#[test]
fn merge_single_force_merges_failed_task_with_committed_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-force-failed", repo.path(), wt.path(), &branch);
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-force-failed", false, false, true, None, true);
    assert!(result.is_ok(), "force merge failed: {result:?}");

    let loaded = store.get_task("t-force-failed").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
    assert!(repo.path().join("agent-work.txt").exists());
    let events = store.get_events("t-force-failed").unwrap();
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Error
            && event.detail
                == "Force-merged task t-force-failed from status FAIL — agent/tests did \
            not complete successfully"
    }));
}

#[test]
fn force_merge_warning_uses_latest_verification_status() {
    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree(
        "t-force-latest-verify",
        Path::new("."),
        Path::new("/tmp/missing-worktree"),
        "branch",
    );
    store.insert_task(&task).unwrap();
    store
        .update_verify_status(task.id.as_str(), VerifyStatus::Failed)
        .unwrap();

    record_force_merge_warning(&store, &task).unwrap();

    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| { event.detail.contains("verification command failed") })
    );
}

#[test]
fn merge_single_force_rejects_failed_task_without_commits() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_empty_worktree_branch(repo.path());
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-force-empty", repo.path(), wt.path(), &branch);
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-force-empty", false, false, true, None, true);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No commits to merge"),
        "unexpected error: {err}"
    );

    let loaded = store.get_task("t-force-empty").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Failed);
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

#[test]
fn merge_single_works_without_worktree_branch() {
    let _permit = test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let task = Task {
        id: TaskId("t-inplace".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    };
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-inplace", false, false, false, None, true);
    assert!(result.is_ok());
    let loaded = store.get_task("t-inplace").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
}

#[test]
fn merge_single_preserves_worktree_on_conflict() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("conflict");
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
    // Create conflicting changes
    std::fs::write(wt.path().join("init.txt"), "branch\n").unwrap();
    git(wt.path(), &["add", "init.txt"]);
    git(wt.path(), &["commit", "-m", "branch"]);
    std::fs::write(repo.path().join("init.txt"), "main\n").unwrap();
    git(repo.path(), &["add", "init.txt"]);
    git(repo.path(), &["commit", "-m", "main"]);

    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-conflict", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-conflict", false, false, false, None, true);
    assert!(result.is_err());
    // Worktree must be preserved for manual resolution
    assert!(wt.path().exists());
    // Task must stay Done
    let loaded = store.get_task("t-conflict").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);

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
fn merge_single_without_repo_path_resolves_from_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    // Simulate the old bug: repo_path is None
    let mut task = make_task_with_worktree("t-no-repo", repo.path(), wt.path(), &branch);
    task.repo_path = None;
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-no-repo", false, false, false, None, true);
    assert!(
        result.is_ok(),
        "merge should resolve repo from worktree: {result:?}"
    );
    assert!(repo.path().join("agent-work.txt").exists());
}

#[test]
fn merge_single_merges_into_target_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let target = unique("target");
    git(repo.path(), &["branch", &target]);
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-target", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single_with_output(&store, "t-target", false, false, false, Some(&target), true);
    assert!(result.is_ok(), "merge_single failed: {result:?}");

    let current = Command::new("git")
        .args([
            "-C",
            &repo.path().to_string_lossy(),
            "branch",
            "--show-current",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), target);
    assert!(repo.path().join("agent-work.txt").exists());

    git(repo.path(), &["checkout", "main"]);
    assert!(!repo.path().join("agent-work.txt").exists());
}
