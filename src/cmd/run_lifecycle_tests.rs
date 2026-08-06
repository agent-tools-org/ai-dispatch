// Tests for post-run lifecycle dirty-state safeguards.
// Covers final dirty worktree assertion behavior around task status/events.
// Deps: run_lifecycle helpers, Store, git CLI, tempfile.

use super::{
    RunArgs, final_dirty_assertion,
    run_dirty::{DirtyWorktreeAction, post_agent_dirty_worktree_cleanup},
    run_lifecycle::{LifecycleMode, post_run_lifecycle},
    run_prompt::PromptBundle,
};
use crate::{hooks::Hook, store::Store, test_subprocess, types::*};
use chrono::Local;
use std::{path::Path, process::Command, sync::Arc};

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init"]);
    dir
}

fn write_path(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(file, content).unwrap();
}

fn prompt_bundle() -> PromptBundle {
    PromptBundle { effective_prompt: "prompt".to_string(), context_files: Vec::new(), prompt_tokens: 0, injected_memory_ids: Vec::new() }
}

fn task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
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

#[test]
fn final_assertion_fails_task_when_worktree_still_dirty() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-dirty".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "src/lib.rs", "pub fn value() -> u8 { 1 }\n");

    let failed = final_dirty_assertion(&store, &task_id, dir.path().to_str().unwrap(), false, None).unwrap();

    assert!(failed);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.detail.contains("FAIL: agent left uncommitted changes after rescue and retry")));
    assert!(events.iter().any(|event| event.detail.contains("?? src/lib.rs")));
}

#[test]
fn final_assertion_skipped_for_read_only() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-readonly".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "result-t-readonly.md", "audit output\n");

    let failed = final_dirty_assertion(&store, &task_id, dir.path().to_str().unwrap(), true, None).unwrap();

    assert!(!failed);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(!events.iter().any(|event| event.detail.contains("FAIL: agent left uncommitted changes")));
}

#[test]
fn final_assertion_ignores_dirty_files_in_baseline() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-baseline".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "src/user.rs", "pub fn user() {}\n");
    let baseline = vec!["?? src/user.rs".to_string()];

    let failed = final_dirty_assertion(
        &store,
        &task_id,
        dir.path().to_str().unwrap(),
        false,
        Some(&baseline),
    )
    .unwrap();

    assert!(!failed);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
}

#[test]
fn final_assertion_fails_for_dirty_files_outside_baseline() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-outside-baseline".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "src/user.rs", "pub fn user() {}\n");
    write_path(dir.path(), "src/agent.rs", "pub fn agent() {}\n");
    let baseline = vec!["?? src/user.rs".to_string()];

    let failed = final_dirty_assertion(
        &store,
        &task_id,
        dir.path().to_str().unwrap(),
        false,
        Some(&baseline),
    )
    .unwrap();

    assert!(failed);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.detail.contains("?? src/agent.rs")));
    assert!(!events.iter().any(|event| event.detail.contains("?? src/user.rs")));
}

#[tokio::test]
async fn audit_report_mode_skips_dirty_worktree_enforcement() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-audit".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "src/user.rs", "pub fn user() {}\n");
    let args = RunArgs {
        audit_report_mode: true,
        ..Default::default()
    };

    let action = post_agent_dirty_worktree_cleanup(
        &store,
        &task_id,
        &args,
        dir.path().to_str().unwrap(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(action, DirtyWorktreeAction::Continue);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
    assert!(!Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["rev-parse", "--verify", "HEAD"])
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn background_lifecycle_runs_verify_and_checklist_scan() {
    let _permit = test_subprocess::acquire();
    let aid_home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-bg-lifecycle-ok".to_string());
    let output_path = dir.path().join("output.md");
    std::fs::write(&output_path, "implemented alpha\n").unwrap();
    let mut stored = task(task_id.as_str(), TaskStatus::Done);
    stored.output_path = Some(output_path.to_string_lossy().to_string());
    store.insert_task(&stored).unwrap();
    let args = RunArgs {
        verify: Some("true".to_string()),
        dir: Some(dir.path().to_string_lossy().to_string()),
        checklist: vec!["alpha".to_string(), "beta".to_string()],
        ..Default::default()
    };

    post_run_lifecycle(
        LifecycleMode::Background,
        &store,
        &task_id,
        &args,
        AgentKind::Codex,
        "codex",
        args.dir.as_ref(),
        None,
        None,
        None,
        &[],
        &prompt_bundle(),
        TaskStatus::Done,
        None,
    )
    .await
    .unwrap();

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Passed);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.detail.contains("Checklist:")));
}

#[tokio::test]
async fn background_lifecycle_runs_on_fail_hook() {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-bg-lifecycle-fail".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Failed)).unwrap();
    let hook_path = dir.path().join("hook.txt");
    let hook = Hook::new_trusted(
        "on_fail".to_string(),
        format!("printf failed > '{}'", hook_path.display()),
        None,
    );

    post_run_lifecycle(
        LifecycleMode::Background,
        &store,
        &task_id,
        &RunArgs::default(),
        AgentKind::Codex,
        "codex",
        None,
        None,
        None,
        None,
        &[hook],
        &prompt_bundle(),
        TaskStatus::Failed,
        None,
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(hook_path).unwrap(), "failed");
}
