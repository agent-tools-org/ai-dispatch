// Adoption outcome tests: unobserved detach must not read as success.
// Covers store judgment, board, show, and completion notifications.
// Deps: background reaper helpers, board/show/notify, Store.

use std::sync::Arc;

use chrono::{Duration, Local};

use super::{check_zombie_tasks_with, save_spec, BackgroundRunSpec};
use crate::paths;
use crate::store::Store;
use crate::types::outcome::UnverifiedReason;
use crate::types::{
    AgentKind, EventKind, Task, TaskEvent, TaskId, TaskOutcome, TaskStatus, VerifyStatus,
};

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
        created_at: Local::now() - Duration::seconds(5),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: true,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

fn spec(id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: id.to_string(),
        worker_pid: Some(77),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        result_file: None,
        model: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: Some(60),
        idle_timeout_secs: Some(3600),
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        hooks: vec![],
        template: None,
        worktree: None,
        base_branch: None,
        peer_review: None,
        audit: false,
        scope: vec![],
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: Some(999999),
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
        detached: true,
    }
}

struct TestHome {
    _temp: tempfile::TempDir,
    _guard: paths::AidHomeGuard,
}

fn setup_home() -> TestHome {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    TestHome {
        _temp: temp,
        _guard: guard,
    }
}

fn adopt_dead_detached(store: &Store, id: &str) {
    store.insert_task(&task(id, TaskStatus::Running)).expect("insert");
    save_spec(&spec(id)).expect("save spec");
    let cleaned = check_zombie_tasks_with(store, |_| false).expect("reap");
    assert_eq!(cleaned, vec![id.to_string()]);
}

#[test]
fn unobserved_detach_is_not_success_on_operator_surfaces() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    adopt_dead_detached(&store, "t-detach-done");

    let loaded = store.get_task("t-detach-done").expect("get").expect("task");
    // Old behaviour recorded Done and left verify_status=Skipped, which derives
    // Delivered (success) when no verify command was set. This assertion is the
    // one that fails if that Done branch is restored.
    assert!(!loaded.outcome().is_success(), "unobserved exit must not be success");
    assert_eq!(
        loaded.outcome(),
        TaskOutcome::Unverified(UnverifiedReason::NoResult)
    );
    assert_eq!(loaded.verify_status, VerifyStatus::Unobserved);
    assert_eq!(loaded.status, TaskStatus::Done);

    let board = crate::board::render_board(&[loaded.clone()], &store).expect("board");
    assert!(board.contains("[VNORESULT]"), "board: {board}");
    assert!(board.contains("0 done"), "board must not count this as done: {board}");

    let show_store = Arc::new(Store::open_memory().expect("show store"));
    show_store.insert_task(&loaded).expect("insert show");
    let summary = crate::cmd::show::summary_text(&show_store, "t-detach-done").expect("show");
    assert!(summary.contains("[VNORESULT]"), "show: {summary}");

    crate::notify::notify_completion(&loaded);
    let jsonl = std::fs::read_to_string(paths::aid_dir().join("completions.jsonl")).expect("jsonl");
    let event: serde_json::Value = serde_json::from_str(jsonl.lines().next().expect("line")).expect("json");
    assert_eq!(event["status"], "DONE");
    assert_eq!(event["outcome"], "unverified");
    assert_eq!(event["verify_status"], "unobserved");
    assert_ne!(event["outcome"], "delivered");

    let events = store.get_events("t-detach-done").expect("events");
    assert!(events.iter().any(|e| e.detail.contains("unobserved")));
    assert!(
        !events
            .iter()
            .any(|e| e.event_kind == EventKind::Completion && e.detail.contains("unobserved")),
        "adoption must not write a Completion event for an unobserved exit"
    );
}

#[test]
fn records_done_when_detached_agent_has_observed_completion() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-detach-obs", TaskStatus::Running))
        .expect("insert task");
    save_spec(&spec("t-detach-obs")).expect("save spec");
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-detach-obs".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Completion,
            detail: "completed with 100 tokens".to_string(),
            metadata: None,
        })
        .expect("insert completion event");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");
    assert_eq!(cleaned, vec!["t-detach-obs".to_string()]);
    let loaded = store.get_task("t-detach-obs").expect("get").expect("task");
    assert_eq!(loaded.status, TaskStatus::Done);
    assert_eq!(loaded.verify_status, VerifyStatus::Pending);
    assert!(loaded.outcome().is_success());
    let events = store.get_events("t-detach-obs").expect("events");
    assert!(events.iter().any(|e| e.detail.contains("completion observed")));
}

#[cfg(unix)]
#[test]
fn sigkilled_detached_agent_does_not_read_as_success() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-kill-detach", TaskStatus::Running))
        .expect("insert task");
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn agent");
    let agent_pid = child.id();
    let mut s = spec("t-kill-detach");
    s.agent_pid = Some(agent_pid);
    save_spec(&s).expect("save spec");

    let _ = std::process::Command::new("kill")
        .args(["-9", &agent_pid.to_string()])
        .status();
    let _ = child.wait();

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");
    assert_eq!(cleaned, vec!["t-kill-detach".to_string()]);
    let loaded = store.get_task("t-kill-detach").expect("get").expect("task");
    assert_eq!(loaded.status, TaskStatus::Done);
    assert!(!loaded.outcome().is_success());
    assert_eq!(loaded.verify_status, VerifyStatus::Unobserved);
    assert_eq!(
        loaded.outcome(),
        TaskOutcome::Unverified(UnverifiedReason::NoResult)
    );
}
