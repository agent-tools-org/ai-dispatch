// Tests for dispatch preparation result-file defaults.
// Exports: none.
// Deps: super::prepare_dispatch, crate::store, RunArgs.

use super::*;
use crate::cmd::run::apply_retry_target;
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn isolated_home() -> crate::paths::AidHomeGuard {
    let temp = tempfile::tempdir().unwrap();
    crate::paths::AidHomeGuard::set(temp.path())
}

fn test_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "seed".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Pending,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
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
        verify: None,
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

fn git_output(repo_dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

#[test]
fn report_mode_dirty_skip_uses_narrow_predicate() {
    use crate::agent::classifier::TaskCategory;

    assert!(crate::cmd::report_mode::skips_dirty_enforcement(
        "Cross-audit the nonce fix",
        false,
        TaskCategory::Research,
    ));
    assert!(!crate::cmd::report_mode::skips_dirty_enforcement(
        "review and fix the parser bug",
        false,
        TaskCategory::ComplexImpl,
    ));
    assert!(crate::cmd::report_mode::skips_dirty_enforcement(
        "anything",
        true,
        TaskCategory::ComplexImpl,
    ));
    assert!(crate::cmd::report_mode::skips_dirty_enforcement(
        "code review of module X",
        false,
        TaskCategory::Research,
    ));
}

#[test]
fn generated_id_collision_retries_and_dispatch_succeeds() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&test_task("t-00000001")).unwrap();
    TaskId::set_generate_sequence_for_tests(&["t-00000001", "t-00000002"]);
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Investigate a concrete task routing bug.".to_string(),
        ..Default::default()
    };

    let prepared = prepare_dispatch_with(&store, &mut args, |_| true).unwrap();

    assert_eq!(prepared.task_id.as_str(), "t-00000002");
    assert!(store.get_task("t-00000001").unwrap().is_some());
    assert!(store.get_task("t-00000002").unwrap().is_some());
}

#[test]
fn generated_id_exhaustion_does_not_reset_worktree_branch() {
    let _guard = isolated_home();
    let _permit = crate::test_subprocess::acquire();
    let repo = init_repo();
    let store = Arc::new(Store::open_memory().unwrap());
    let branch = "fix/collision-branch";
    git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("sentinel.txt"), "keep\n").unwrap();
    git(repo.path(), &["add", "sentinel.txt"]);
    git(repo.path(), &["commit", "-m", "sentinel"]);
    let branch_before = git_output(repo.path(), &["rev-parse", branch]);
    git(repo.path(), &["checkout", "main"]);
    let ids = [
        "t-dead0001", "t-dead0002", "t-dead0003", "t-dead0004",
        "t-dead0005", "t-dead0006", "t-dead0007", "t-dead0008",
    ];
    for id in ids {
        store.insert_task(&test_task(id)).unwrap();
    }
    TaskId::set_generate_sequence_for_tests(&ids);
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Investigate a concrete task routing bug.".to_string(),
        repo_root: Some(repo.path().display().to_string()),
        worktree: Some(branch.to_string()),
        base_branch: Some("main".to_string()),
        ..Default::default()
    };

    let err = match prepare_dispatch_with(&store, &mut args, |_| true) {
        Ok(_) => panic!("dispatch should fail after generated ID retries are exhausted"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("failed to allocate unique task ID"));
    assert_eq!(git_output(repo.path(), &["rev-parse", branch]), branch_before);
}

#[test]
fn retry_prepare_persists_live_worktree_metadata() {
    let _guard = isolated_home();
    let _permit = crate::test_subprocess::acquire();
    let repo = init_repo();
    let store = Arc::new(Store::open_memory().unwrap());
    let branch = "fix/retry-live-persist";
    let worktree = crate::worktree::create_worktree(repo.path(), branch, Some("main")).unwrap();
    let worktree_path = worktree.path.to_string_lossy().to_string();
    let mut parent = test_task("t-parent-retry-live");
    parent.repo_path = Some(repo.path().display().to_string());
    parent.worktree_path = Some(worktree_path.clone());
    parent.worktree_branch = Some(branch.to_string());
    store.insert_task(&parent).unwrap();
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Retry the prior task.".to_string(),
        parent_task_id: Some(parent.id.to_string()),
        ..Default::default()
    };

    apply_retry_target(&parent, &mut args).unwrap();
    let prepared = prepare_dispatch_with(&store, &mut args, |_| true).unwrap();

    let saved = store.get_task(prepared.task_id.as_str()).unwrap().unwrap();
    assert_eq!(saved.worktree_path.as_deref(), Some(worktree_path.as_str()));
    assert_eq!(saved.worktree_branch.as_deref(), Some(branch));
}

#[test]
fn prepare_dispatch_updates_log_path_when_id_is_auto_suffixed() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut first = RunArgs {
        agent_name: "codex".to_string(),
        existing_task_id: Some(TaskId("t-ebcf".to_string())),
        prompt: "Investigate a concrete task routing bug.".to_string(),
        ..Default::default()
    };
    prepare_dispatch_with(&store, &mut first, |_| true).unwrap();

    let mut second = RunArgs {
        agent_name: "codex".to_string(),
        existing_task_id: Some(TaskId("t-ebcf".to_string())),
        prompt: "Investigate a concrete task routing bug again.".to_string(),
        ..Default::default()
    };
    let prepared = prepare_dispatch_with(&store, &mut second, |_| true).unwrap();

    let expected = crate::paths::log_path("t-ebcf-2");
    let saved = store.get_task("t-ebcf-2").unwrap().unwrap();
    assert_eq!(prepared.task_id.as_str(), "t-ebcf-2");
    assert_eq!(prepared.log_path, expected);
    assert_eq!(saved.log_path.as_deref(), Some(expected.to_str().unwrap()));
}

#[test]
fn prepare_dispatch_uses_task_specific_audit_result_file() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Read-only audit of branch X.".to_string(),
        ..Default::default()
    };

    let prepared = prepare_dispatch_with(&store, &mut args, |_| true).unwrap();

    assert_eq!(
        args.result_file.as_deref(),
        Some(crate::cmd::report_mode::task_result_file(prepared.task_id.as_str()).as_str())
    );
    assert!(!args.audit_report_mode);
}

#[test]
fn prepare_dispatch_skips_auto_result_file_when_output_is_set() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Review the implementation and list findings.".to_string(),
        read_only: true,
        output: Some("audit.md".to_string()),
        ..Default::default()
    };

    prepare_dispatch_with(&store, &mut args, |_| true).unwrap();

    assert_eq!(args.result_file, None);
}

#[test]
fn prepare_dispatch_keeps_write_intent_out_of_report_mode() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Add unit tests for the read-only audit module".to_string(),
        ..Default::default()
    };

    prepare_dispatch_with(&store, &mut args, |_| true).unwrap();

    assert!(!args.audit_report_mode);
    assert_eq!(args.result_file, None);
}

#[test]
fn prepare_dispatch_rejects_requested_worktree_when_it_is_the_repo_root() {
    let _guard = isolated_home();
    let _permit = crate::test_subprocess::acquire();
    let repo = init_repo();
    let store = Arc::new(Store::open_memory().unwrap());
    git(repo.path(), &["checkout", "-b", "chore/main-dispatch"]);
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Investigate a concrete task routing bug.".to_string(),
        repo_root: Some(repo.path().display().to_string()),
        worktree: Some("chore/main-dispatch".to_string()),
        ..Default::default()
    };

    let err = match prepare_dispatch(&store, &mut args) {
        Ok(prepared) => panic!(
            "dispatch should reject repo-root worktree: repo={:?} worktree={:?}",
            prepared.repo_path, prepared.wt_path
        ),
        Err(err) => err,
    };

    assert!(err.to_string().contains("main working tree"));
}
