// Tests for foreground run specs and interruption cleanup.
// Covers spec guard removal and signal cleanup side effects.
// Deps: run_foreground_guard internals, Store, paths, and task types.
use super::*;
use crate::paths;
use crate::types::{AgentKind, Task, TaskStatus, VerifyStatus};

fn make_spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(123),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: None,
        output: None,
        result_file: None,
        model: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs: Some(60),
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        template: None,
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: Some(456),
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
    }
}

fn make_task(task_id: &str) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
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
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn foreground_spec_guard_clears_spec_on_drop() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let guard = ForegroundSpecGuard::save(make_spec("t-fg-clear")).expect("save spec");
    assert!(paths::job_path("t-fg-clear").exists());

    drop(guard);

    assert!(!paths::job_path("t-fg-clear").exists());
}

#[test]
fn interrupt_cleanup_records_failed_kills_agent_and_clears_spec() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-fg-int");
    store.insert_task(&task).expect("insert task");
    background::save_spec(&make_spec("t-fg-int")).expect("save spec");
    let mut killed = Vec::new();

    handle_foreground_interrupt_with(&store, &task.id, "SIGTERM", |pid| killed.push(pid))
        .expect("interrupt cleanup");

    assert_eq!(killed, vec![456]);
    assert!(!paths::job_path("t-fg-int").exists());
    assert_eq!(
        store.get_task("t-fg-int").expect("get task").expect("task").status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-fg-int").expect("events");
    assert!(events.iter().any(|event| event.detail == "interrupted by signal SIGTERM"));
}
