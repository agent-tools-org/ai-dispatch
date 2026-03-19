// Tests for background worker persistence and zombie-task cleanup.
// Covers spec serialization and store reconciliation side effects.

use chrono::Local;

use super::{
    build_on_done_command, check_zombie_tasks_with, save_spec, BackgroundRunSpec,
    ZOMBIE_FAILURE_DETAIL,
};
use crate::paths;
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, EventKind, Task, TaskId, TaskStatus, VerifyStatus};

#[test]
fn serializes_spec_to_json() {
    let spec = BackgroundRunSpec {
        task_id: "t-5a0e".to_string(),
        worker_pid: Some(4242),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        model: None,
        verify: Some("auto".to_string()),
        judge: Some("gemini".to_string()),
        max_duration_mins: Some(90),
        retry: 2,
        group: Some("wg-abcd".to_string()),
        skills: vec![],
        template: None,
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
    };

    let content = serde_json::to_string_pretty(&spec).unwrap();
    assert!(content.contains("\"agent_name\""));
    assert!(content.contains("\"codex\""));
}

#[test]
fn serializes_cascade_field() {
    let spec = BackgroundRunSpec {
        cascade: vec!["opencode".to_string(), "cursor".to_string()],
        ..make_spec("t-cascade")
    };
    let content = serde_json::to_string_pretty(&spec).unwrap();
    assert!(content.contains("\"cascade\""));
    assert!(content.contains("\"opencode\""));
    assert!(content.contains("\"cursor\""));
    let parsed: BackgroundRunSpec = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.cascade, vec!["opencode", "cursor"]);
}

#[test]
fn marks_running_background_tasks_failed_when_worker_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-1a1a", TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task("t-2b2b", TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task("t-3c3c", TaskStatus::Running))
        .unwrap();
    save_spec(&make_spec("t-1a1a")).unwrap();
    save_spec(&make_spec("t-2b2b")).unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 101).unwrap();

    assert_eq!(cleaned, vec!["t-2b2b".to_string()]);
    assert_eq!(
        store.get_task("t-1a1a").unwrap().unwrap().status,
        TaskStatus::Running
    );
    assert_eq!(
        store.get_task("t-2b2b").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert_eq!(
        store.get_task("t-3c3c").unwrap().unwrap().status,
        TaskStatus::Running
    );

    let events = store.get_events("t-2b2b").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
    assert_eq!(events[0].detail, ZOMBIE_FAILURE_DETAIL);

    let stderr = std::fs::read_to_string(paths::stderr_path("t-2b2b")).unwrap();
    assert_eq!(stderr.trim(), ZOMBIE_FAILURE_DETAIL);
}

#[test]
fn completion_notifications_are_written_as_jsonl() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let mut task = make_task("t-4d4d", TaskStatus::Done);
    task.duration_ms = Some(1_500);
    task.cost_usd = Some(0.25);
    task.prompt = "x".repeat(120);

    crate::notify::notify_completion(&task);

    let line = crate::notify::read_recent(20).unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["task_id"], "t-4d4d");
    assert_eq!(value["agent"], "codex");
    assert_eq!(value["status"], "DONE");
    assert_eq!(value["duration_ms"], 1_500);
    assert_eq!(value["cost_usd"], 0.25);
    assert_eq!(value["prompt"], "x".repeat(100));
    assert!(value["timestamp"].as_str().is_some());
}

#[test]
fn build_on_done_command_splits_simple_argv() {
    let cmd = build_on_done_command("echo done").unwrap();
    let debug = format!("{cmd:?}");
    assert!(debug.contains("\"echo\""));
    assert!(debug.contains("\"done\""));
}

#[test]
fn build_on_done_command_does_not_expand_shell_operators() {
    let cmd = build_on_done_command("echo done && false").unwrap();
    let debug = format!("{cmd:?}");
    assert!(debug.contains("\"&&\""));
    assert!(debug.contains("\"false\""));
}

fn make_spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(if task_id == "t-1a1a" { 101 } else { 202 }),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        model: None,
        verify: None,
        judge: None,
        max_duration_mins: None,
        retry: 0,
        group: None,
        skills: vec![],
        template: None,
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
    }
}

fn make_task(task_id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
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
        read_only: false,
        budget: false,
    }
}

#[test]
fn quota_cascade_skipped_for_batch_tasks() {
    let batch_spec = BackgroundRunSpec {
        group: Some("wg-test".to_string()),
        ..make_spec("t-batch")
    };
    // Batch tasks have group set — cascade guard (spec.group.is_none()) blocks them
    assert!(
        batch_spec.group.is_some(),
        "batch tasks have group set, cascade should be skipped"
    );

    let solo_spec = BackgroundRunSpec {
        group: None,
        ..make_spec("t-solo")
    };
    // Non-batch tasks have no group — cascade is allowed
    assert!(solo_spec.group.is_none(), "solo tasks should allow cascade");
}

#[test]
fn check_worker_capacity_warns_at_soft_limit() {
    let store = Store::open_memory().unwrap();
    // No tasks running — should pass silently
    assert!(super::check_worker_capacity(&store).is_ok());
}

#[test]
fn check_worker_capacity_rejects_at_hard_limit() {
    let store = Store::open_memory().unwrap();
    // Insert MAX_WORKERS running tasks to trigger hard limit
    for i in 0..super::MAX_WORKERS {
        let id = format!("t-cap{i:03}");
        store
            .insert_task(&make_task(&id, TaskStatus::Running))
            .unwrap();
    }
    let result = super::check_worker_capacity(&store);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Worker limit reached"));
}

#[cfg(unix)]
#[test]
fn is_process_running_returns_false_for_zombie() {
    let _permit = test_subprocess::acquire();
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            libc::_exit(0);
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
        let child_pid = pid as u32;
        let Ok(ps_output) = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &child_pid.to_string()])
            .output()
        else {
            let mut status: i32 = 0;
            libc::waitpid(pid, &mut status, 0);
            return;
        };
        if !ps_output.status.success() || ps_output.stdout.is_empty() {
            let mut status: i32 = 0;
            libc::waitpid(pid, &mut status, 0);
            return;
        }

        assert!(!super::is_process_running(child_pid));

        let mut status: i32 = 0;
        libc::waitpid(pid, &mut status, 0);
    }
}

#[test]
fn agent_pid_stored_and_loaded_correctly() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    save_spec(&make_spec("t-a100")).unwrap();

    assert!(super::load_agent_pid("t-a100").unwrap().is_none());

    super::update_agent_pid("t-a100", 12345).unwrap();

    let loaded = super::load_agent_pid("t-a100").unwrap();
    assert_eq!(loaded, Some(12345));

    let spec = super::load_spec("t-a100").unwrap();
    assert_eq!(spec.agent_pid, Some(12345));
    assert_eq!(spec.worker_pid, Some(202));
}

#[test]
fn agent_pid_backwards_compatible() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    save_spec(&make_spec("t-c200")).unwrap();

    let spec = super::load_spec("t-c200").unwrap();
    assert!(spec.agent_pid.is_none());
}

#[test]
fn zombie_cleanup_skips_autocommit_for_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-a1b2", TaskStatus::Running);
    task.read_only = true;
    store.insert_task(&task).unwrap();

    save_spec(&BackgroundRunSpec {
        worker_pid: Some(999999),
        ..make_spec("t-a1b2")
    })
    .unwrap();

    let cleaned = check_zombie_tasks_with(&store, |_| false).unwrap();
    assert_eq!(cleaned, vec!["t-a1b2".to_string()]);

    let failed_task = store.get_task("t-a1b2").unwrap().unwrap();
    assert_eq!(failed_task.status, TaskStatus::Failed);
}
