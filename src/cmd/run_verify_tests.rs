// Tests for verify helper output excerpts and dependency hints.
// Exports: none.
// Deps: super, crate::store, crate::types, tempfile.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn make_task(id: &str, worktree_path: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: Some(worktree_path.to_string()),
        worktree_branch: Some("feat/test".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: Some("false".to_string()),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
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

#[test]
fn verify_output_excerpt_keeps_last_lines() {
    let output = (1..=10)
        .map(|idx| format!("line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");

    let excerpt = verify_output_excerpt(&output).unwrap();

    assert_eq!(
        excerpt,
        "line 3 | line 4 | line 5 | line 6 | line 7 | line 8 | line 9 | line 10"
    );
}

#[test]
fn maybe_verify_records_missing_deps_hint_for_fresh_worktree() {
    let store = Store::open_memory().unwrap();
    let worktree = TempDir::new().unwrap();
    let worktree_str = worktree.path().to_string_lossy().to_string();
    crate::worktree_deps::prepare_worktree_dependencies(
        &store, &TaskId("t-verify-hint".to_string()), worktree.path(), worktree.path(),
        None, false, None, true, None,
    )
    .unwrap();
    let script = worktree.path().join("missing-deps.sh");
    std::fs::write(&script, "#!/bin/sh\necho \"Error: Cannot find module 'vite'\" >&2\nexit 1\n").unwrap();
    store
        .insert_task(&make_task("t-verify-hint", &worktree_str))
        .unwrap();

    maybe_verify_impl(
        &store,
        &TaskId("t-verify-hint".to_string()),
        Some(&format!("sh {}", script.display())),
        Some(&worktree_str),
        None,
    );

    let events = store.get_events("t-verify-hint").unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("verify likely failed because dependencies weren't installed")
    }));
}

#[test]
fn maybe_verify_reports_stale_worktree_when_dir_is_missing() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-stale-verify".to_string());
    let worktree_path = crate::worktree::aid_worktree_path(Path::new(env!("CARGO_MANIFEST_DIR")), "feat/stale");
    let worktree_path = worktree_path.to_string_lossy().to_string();
    let mut task = make_task("t-stale-verify", &worktree_path);
    task.workgroup_id = Some("wg-stale".to_string());
    task.worktree_branch = Some("feat/stale".to_string());
    task.verify = Some("auto".to_string());
    task.verify_status = VerifyStatus::Skipped;
    store.insert_task(&task).unwrap();

    maybe_verify_impl(
        &store,
        &task_id,
        Some("auto"),
        Some(&format!("{worktree_path}/.aid/batches")),
        None,
    );

    let error = store.latest_error(task_id.as_str()).unwrap();
    assert!(error.contains("batch file / task dir missing in worktree"));
    assert!(error.contains("principal acceptance") && error.contains("custody GC"));
}

#[test]
fn fast_fail_cleanup_allows_legacy_tmp_worktree_path() {
    let store = Store::open_memory().unwrap();
    let repo = TempDir::new().unwrap();
    init_repo(repo.path());
    // Unique /tmp/aid-wt-* path (sandbox prefix); guard owns teardown on panic too.
    let path_holder = tempfile::Builder::new()
        .prefix("aid-wt-fast-fail-")
        .tempdir_in("/tmp")
        .unwrap();
    let worktree_path = path_holder.path().to_path_buf();
    drop(path_holder);
    let _guard = crate::test_env::TmpWorktreeGuard::with_repo(repo.path(), worktree_path.clone());
    git(repo.path(), &["worktree", "add", &worktree_path.to_string_lossy(), "-b", "feat/fast-fail-legacy"]);
    let task_id = TaskId("t-fast-fail-legacy".to_string());
    let mut task = make_task(task_id.as_str(), &worktree_path.to_string_lossy());
    task.status = TaskStatus::Failed;
    task.duration_ms = Some(100);
    task.repo_path = Some(repo.path().to_string_lossy().to_string());
    task.worktree_branch = Some("feat/fast-fail-legacy".to_string());
    store.insert_task(&task).unwrap();

    maybe_cleanup_fast_fail_impl(&store, &task_id, &task);

    assert!(worktree_path.exists());
}

#[test]
fn fast_fail_cleanup_rejects_non_aid_path() {
    let store = Store::open_memory().unwrap();
    let worktree = TempDir::new().unwrap();
    let task_id = TaskId("t-fast-fail-non-aid".to_string());
    let mut task = make_task(task_id.as_str(), &worktree.path().to_string_lossy());
    task.status = TaskStatus::Failed;
    task.duration_ms = Some(100);
    store.insert_task(&task).unwrap();

    maybe_cleanup_fast_fail_impl(&store, &task_id, &task);

    assert!(worktree.path().exists());
}

#[test]
fn retry_target_application_replaces_stale_worktree_dir_with_repo() {
    let stale_dir = "/tmp/aid-stale-worktree-dir";
    let repo = TempDir::new().unwrap();
    init_repo(repo.path());
    git(repo.path(), &["branch", "chore/retry-stale"]);
    let repo_dir = repo.path().to_string_lossy().to_string();
    let mut task = make_task("t-stale-retry", stale_dir);
    task.repo_path = Some(repo_dir.clone());
    task.worktree_branch = Some("chore/retry-stale".to_string());
    let mut retry_args = RunArgs {
        dir: Some(stale_dir.to_string()),
        worktree: Some("old-worktree".to_string()),
        ..Default::default()
    };

    super::super::apply_retry_target(&task, &mut retry_args).unwrap();

    assert_eq!(retry_args.dir.as_deref(), Some(repo_dir.as_str()));
    assert_eq!(retry_args.worktree.as_deref(), Some("chore/retry-stale"));
}

#[test]
fn retry_target_application_preserves_live_worktree_across_generations() {
    // Real repo + linked worktree: the isolation guard resolves the main checkout via git
    // and fails closed on paths it cannot place, so plain temp dirs will not do here.
    let repo = TempDir::new().unwrap();
    init_repo(repo.path());
    let worktree_root = TempDir::new().unwrap();
    let worktree_path = worktree_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", "chore/retry-live", &worktree_path.to_string_lossy()]);
    let repo_dir = repo.path().to_string_lossy().to_string();
    let worktree_dir = worktree_path.to_string_lossy().to_string();
    let mut first = make_task("t-live-retry", &worktree_dir);
    first.repo_path = Some(repo_dir.clone());
    first.worktree_branch = Some("chore/retry-live".to_string());
    let mut first_retry = RunArgs {
        dir: Some(repo_dir.clone()),
        ..Default::default()
    };

    super::super::apply_retry_target(&first, &mut first_retry).unwrap();

    assert_eq!(first_retry.dir.as_deref(), Some(worktree_dir.as_str()));
    assert_eq!(first_retry.worktree.as_deref(), Some("chore/retry-live"));

    let mut second = make_task("t-live-retry-child", &worktree_dir);
    second.repo_path = Some(repo_dir.clone());
    second.worktree_branch = first_retry.worktree.clone();
    let mut second_retry = RunArgs {
        dir: Some(repo_dir.clone()),
        ..Default::default()
    };

    super::super::apply_retry_target(&second, &mut second_retry).unwrap();

    assert_eq!(second_retry.dir.as_deref(), Some(worktree_dir.as_str()));
    assert_eq!(second_retry.worktree.as_deref(), Some("chore/retry-live"));
}

#[test]
fn retry_target_application_errors_without_recoverable_repo() {
    let stale_dir = "/tmp/aid-stale-worktree-dir-without-repo";
    let fallback_dir = TempDir::new().unwrap();
    let fallback_dir = fallback_dir.path().to_string_lossy().to_string();
    let mut task = make_task("t-stale-retry-no-repo", stale_dir);
    task.repo_path = None;
    task.worktree_branch = Some("chore/retry-stale".to_string());
    let mut retry_args = RunArgs {
        dir: Some(fallback_dir.clone()),
        ..Default::default()
    };

    let err = super::super::apply_retry_target(&task, &mut retry_args).unwrap_err();

    assert!(err.to_string().contains("no recoverable repo_path"));
}

#[test]
fn retry_target_application_errors_when_stale_worktree_has_no_repo() {
    let missing_holder = TempDir::new().unwrap();
    let stale_dir = missing_holder.path().join("missing-worktree");
    let stale_dir = stale_dir.to_string_lossy().to_string();
    let mut task = make_task("t-stale-retry-no-target", &stale_dir);
    task.repo_path = None;
    task.worktree_branch = Some("chore/retry-stale".to_string());
    let mut retry_args = RunArgs {
        dir: Some(stale_dir),
        worktree: Some("chore/retry-stale".to_string()),
        ..Default::default()
    };

    let err = super::super::apply_retry_target(&task, &mut retry_args).unwrap_err();

    assert!(err.to_string().contains("no recoverable repo_path"));
}

#[test]
fn retry_target_application_errors_without_worktree_or_repo() {
    let mut task = make_task("t-retry-no-target", "/tmp/aid-unused-worktree");
    task.repo_path = None;
    task.worktree_path = None;
    task.worktree_branch = None;
    let mut retry_args = RunArgs::default();

    let err = super::super::apply_retry_target(&task, &mut retry_args).unwrap_err();

    assert!(err.to_string().contains("no usable worktree path, retry dir, or repo path"));
}
