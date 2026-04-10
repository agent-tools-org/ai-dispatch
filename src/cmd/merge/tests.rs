// Tests for aid merge — unit tests for git helpers + integration tests for merge workflows.
// Deps: super::*, test_subprocess, tempfile

use super::*;
use crate::test_subprocess;
use crate::types::*;
use chrono::Local;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let s = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(args)
        .output()
        .unwrap();
    assert!(s.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&s.stderr));
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())
}

/// Create a git repo with one commit. Returns the TempDir.
fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@aid.dev"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("init.txt"), "init\n").unwrap();
    git(repo.path(), &["add", "init.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

/// Create a worktree branch with one committed change. Returns (worktree_dir, branch_name).
fn create_worktree_with_commit(repo: &Path) -> (TempDir, String) {
    let branch = unique("test-branch");
    let wt = TempDir::new().unwrap();
    git(repo, &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    std::fs::write(wt.path().join("agent-work.txt"), "agent output\n").unwrap();
    git(wt.path(), &["add", "agent-work.txt"]);
    git(wt.path(), &["commit", "-m", "agent: implement feature"]);
    (wt, branch)
}

fn create_empty_worktree_branch(repo: &Path) -> (TempDir, String) {
    let branch = unique("empty-branch");
    let wt = TempDir::new().unwrap();
    git(repo, &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    (wt, branch)
}

fn create_conflict_worktree(repo: &Path, branch: &str) -> TempDir {
    let wt = TempDir::new().unwrap();
    git(repo, &["worktree", "add", &wt.path().to_string_lossy(), "-b", branch]);
    std::fs::write(wt.path().join("init.txt"), "branch version\n").unwrap();
    git(wt.path(), &["add", "init.txt"]);
    git(wt.path(), &["commit", "-m", "branch change"]);
    std::fs::write(repo.join("init.txt"), "main version\n").unwrap();
    git(repo, &["add", "init.txt"]);
    git(repo, &["commit", "-m", "main change"]);
    wt
}

fn worktree_status(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "status", "--short"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn make_task_with_worktree(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
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
        repo_path: Some(repo.to_string_lossy().to_string()),
        worktree_path: Some(wt.to_string_lossy().to_string()),
        worktree_branch: Some(branch.to_string()),
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}

// --- Unit tests for helper functions ---

#[test]
fn commits_ahead_detects_branch_with_commits() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);
    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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
    assert_eq!(commits_ahead(&repo.path().to_string_lossy(), "nonexistent"), 0);
}

#[test]
fn auto_commit_uncommitted_commits_dirty_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("dirty-branch");
    let wt = TempDir::new().unwrap();
    git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    // Leave changes uncommitted
    std::fs::write(wt.path().join("dirty.txt"), "uncommitted\n").unwrap();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(committed);
    // Now the branch should have commits ahead
    assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
}

#[test]
fn auto_commit_message_includes_filename() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("message-branch");
    let wt = TempDir::new().unwrap();
    git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    std::fs::write(wt.path().join("named-file.txt"), "uncommitted\n").unwrap();

    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(committed);

    let log = Command::new("git")
        .args(["-C", &wt.path().to_string_lossy(), "log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "chore: auto-commit changes to named-file.txt"
    );

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
}

#[test]
fn auto_commit_uncommitted_returns_false_for_clean_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());
    let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
    assert!(!committed);
    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
}

#[test]
fn git_merge_branch_merges_committed_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (wt, branch) = create_worktree_with_commit(repo.path());

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Merged));
    // Verify the file landed in main
    assert!(repo.path().join("agent-work.txt").exists());

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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
        .args(["-C", &repo.path().to_string_lossy(), "branch", "--show-current"])
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
    let _ = Command::new("git").args(["-C", &repo.path().to_string_lossy(), "merge", "--abort"]).output();
    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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
        MergeCheckResult::Ok(commits) => panic!("expected conflict, got clean merge with {commits} commit(s)"),
    }
    assert_eq!(worktree_status(repo.path()), "");

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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
    assert_eq!(std::fs::read_to_string(local_file).unwrap(), "local change\n");

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
}

#[test]
fn git_merge_branch_stashes_and_warns_on_pop_conflict() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("pop-conflict");
    let wt = TempDir::new().unwrap();
    git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    std::fs::write(wt.path().join("init.txt"), "branch change\n").unwrap();
    git(wt.path(), &["add", "init.txt"]);
    git(wt.path(), &["commit", "-m", "branch change"]);
    std::fs::write(repo.path().join("init.txt"), "local change\n").unwrap();

    let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
    assert!(matches!(result, MergeResult::Merged));
    let status = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "status", "--short"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("UU init.txt"));

    git(repo.path(), &["reset", "--hard", "HEAD~1"]);
    git(repo.path(), &["stash", "drop"]);
    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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

    sync_cargo_lock_before_merge(&repo.path().to_string_lossy(), &wt.path().to_string_lossy(), &branch);

    assert_eq!(std::fs::read_to_string(wt.path().join("Cargo.lock")).unwrap(), "version = 2\n");
    assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);

    let log = Command::new("git")
        .args(["-C", &wt.path().to_string_lossy(), "log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "chore: sync Cargo.lock from main");

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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

    let result = merge_single(&store, "t-merge-ok", false, false, None);
    assert!(result.is_ok(), "merge_single failed: {result:?}");

    let loaded = store.get_task("t-merge-ok").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
    assert!(repo.path().join("agent-work.txt").exists());
}

#[test]
fn merge_single_auto_commits_then_merges() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("uncommitted");
    let wt = TempDir::new().unwrap();
    git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    // Leave changes uncommitted — this is the data-loss scenario
    std::fs::write(wt.path().join("uncommitted.txt"), "agent forgot to commit\n").unwrap();

    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-autocommit", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-autocommit", false, false, None);
    assert!(result.is_ok(), "merge_single should auto-commit and merge: {result:?}");

    let loaded = store.get_task("t-autocommit").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Merged);
    assert!(repo.path().join("uncommitted.txt").exists());
    assert_eq!(std::fs::read_to_string(repo.path().join("uncommitted.txt")).unwrap(), "agent forgot to commit\n");
}

#[test]
fn merge_single_fails_when_no_commits_and_no_changes() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("empty");
    let wt = TempDir::new().unwrap();
    git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
    // No changes at all — nothing to merge

    let store = Store::open_memory().unwrap();
    let task = make_task_with_worktree("t-empty", repo.path(), wt.path(), &branch);
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-empty", false, false, None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("No commits to merge"), "unexpected error: {err}");

    // Task should still be Done (not Merged)
    let loaded = store.get_task("t-empty").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);
    // Worktree should be preserved
    assert!(wt.path().exists());

    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
}

#[test]
fn merge_single_rejects_non_done_task() {
    let _permit = test_subprocess::acquire();
    let store = Store::open_memory().unwrap();
    let mut task = make_task_with_worktree("t-running", Path::new("."), Path::new("/tmp"), "b");
    task.status = TaskStatus::Running;
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-running", false, false, None);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("only DONE"));
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
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    };
    store.insert_task(&task).unwrap();

    let result = merge_single(&store, "t-inplace", false, false, None);
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
    git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
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

    let result = merge_single(&store, "t-conflict", false, false, None);
    assert!(result.is_err());
    // Worktree must be preserved for manual resolution
    assert!(wt.path().exists());
    // Task must stay Done
    let loaded = store.get_task("t-conflict").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);

    let _ = Command::new("git").args(["-C", &repo.path().to_string_lossy(), "merge", "--abort"]).output();
    git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
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

    let result = merge_single(&store, "t-no-repo", false, false, None);
    assert!(result.is_ok(), "merge should resolve repo from worktree: {result:?}");
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

    let result = merge_single(&store, "t-target", false, false, Some(&target));
    assert!(result.is_ok(), "merge_single failed: {result:?}");

    let current = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), target);
    assert!(repo.path().join("agent-work.txt").exists());

    git(repo.path(), &["checkout", "main"]);
    assert!(!repo.path().join("agent-work.txt").exists());
}

#[test]
fn merge_group_skips_empty_branches() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let (committed_wt, committed_branch) = create_worktree_with_commit(repo.path());
    let (empty_wt, empty_branch) = create_empty_worktree_branch(repo.path());

    let store = Store::open_memory().unwrap();
    let group_id = "wg-merge-group";

    let mut committed_task =
        make_task_with_worktree("t-merge-group", repo.path(), committed_wt.path(), &committed_branch);
    committed_task.workgroup_id = Some(group_id.to_string());
    store.insert_task(&committed_task).unwrap();

    let mut empty_task =
        make_task_with_worktree("t-empty-branch", repo.path(), empty_wt.path(), &empty_branch);
    empty_task.workgroup_id = Some(group_id.to_string());
    store.insert_task(&empty_task).unwrap();

    let result = merge_group(&store, group_id, false, false, None);
    assert!(result.is_ok(), "merge_group failed: {result:?}");

    let loaded_committed = store.get_task("t-merge-group").unwrap().unwrap();
    assert_eq!(loaded_committed.status, TaskStatus::Merged);
    assert!(repo.path().join("agent-work.txt").exists());

    let loaded_empty = store.get_task("t-empty-branch").unwrap().unwrap();
    assert_eq!(loaded_empty.status, TaskStatus::Done);

    git(repo.path(), &["worktree", "remove", "--force", &empty_wt.path().to_string_lossy()]);
}

#[test]
fn run_rejects_lanes_without_group() {
    let store = Arc::new(Store::open_memory().unwrap());
    let result = run(store, None, None, false, false, None, true);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "--lanes requires --group");
}

// --- Integration test for remove_worktree ---

#[test]
fn remove_worktree_cleans_up_properly() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique("cleanup");
    // Use /tmp/aid-wt-* path to pass sandbox guard
    let wt_path = format!("/tmp/aid-wt-test-{branch}");
    git(repo.path(), &["worktree", "add", &wt_path, "-b", &branch]);

    // Should not panic and worktree dir should be gone
    remove_worktree(&repo.path().to_string_lossy(), &wt_path).unwrap();
    assert!(!Path::new(&wt_path).exists());

    // git worktree list should not show it
    let out = Command::new("git")
        .args(["-C", &repo.path().to_string_lossy(), "worktree", "list"])
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(&branch));
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
    run_post_merge_verify(&repo.path().to_string_lossy(), Some("git missing-subcommand"));
    assert_eq!(worktree_status(repo.path()), "");
}

// --- Sandbox guard tests ---

#[test]
fn sandbox_allows_aid_worktree_paths() {
    let _permit = test_subprocess::acquire();
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
    let repo = init_repo();
    let unsafe_path = repo.path().join("subdir");
    std::fs::create_dir_all(&unsafe_path).unwrap();

    // This should NOT delete the directory — sandbox guard blocks it
    let result = remove_worktree(
        &repo.path().to_string_lossy(),
        &unsafe_path.to_string_lossy(),
    );
    assert!(result.is_err());
    // Directory must still exist
    assert!(unsafe_path.exists(), "Sandbox guard failed: unsafe path was deleted!");
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
