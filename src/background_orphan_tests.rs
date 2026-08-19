// Unit tests for orphaned idle-task reaping and activity bookkeeping filters.
// Covers dead-worker hung detection and ignoring aid self-nudge/ack events.
// Deps: background_orphan helpers, store, and background specs.

use super::*;
use crate::background::{save_spec, BackgroundRunSpec};
use crate::paths;
use crate::store::Store;
use crate::types::{
    AgentKind, EventKind, Task, TaskEvent, TaskStatus, VerifyStatus,
};

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
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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

fn make_spec(task_id: &str, worker_pid: Option<u32>, idle_timeout_secs: Option<u64>) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid,
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None, result_file: None, model: None,
        verify: None, setup: None, iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![], hooks: vec![], template: None,
        worktree: None, base_branch: None, peer_review: None,
        audit: false, scope: vec![],
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
        detached: false,
    }
}

fn insert_event(store: &Store, task_id: &str, age_secs: i64, event_kind: EventKind) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(age_secs),
            event_kind,
            detail: "last progress".to_string(),
            metadata: None,
        })
        .expect("insert event");
}

#[test]
fn orphan_reaper_fails_stale_task_when_worker_is_dead() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-orph1");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_spec("t-orph1", Some(77), Some(120))).expect("save spec");
    insert_event(&store, "t-orph1", 121, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert_eq!(cleaned, vec!["t-orph1".to_string()]);
    assert_eq!(
        store.get_task("t-orph1").expect("get task").expect("task").status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-orph1").expect("events");
    assert!(events.iter().any(|event| event.detail.contains("orphaned supervisor")));
    assert!(events.iter().any(|event| event.detail == "hung_detected"));
}

#[test]
fn orphan_reaper_keeps_stale_task_when_worker_is_alive() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-live1");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_spec("t-live1", Some(77), Some(120))).expect("save spec");
    insert_event(&store, "t-live1", 1_000, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|pid| pid == 77).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-live1").expect("get task").expect("task").status,
        TaskStatus::Running
    );
}

#[test]
fn orphan_reaper_treats_reasoning_as_activity_before_idle_timeout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-idle1");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_spec("t-idle1", Some(77), Some(600))).expect("save spec");
    insert_event(&store, "t-idle1", 500, EventKind::Reasoning);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-idle1").expect("get task").expect("task").status,
        TaskStatus::Running
    );
}

#[test]
fn orphan_reaper_skips_tasks_without_background_spec() {
    let store = Store::open_memory().expect("store");
    let task = make_task("t-nospec");
    store.insert_task(&task).expect("insert task");
    insert_event(&store, "t-nospec", 1_000, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-nospec").expect("get task").expect("task").status,
        TaskStatus::Running
    );
}

#[test]
fn is_stale_requires_idle_timeout_to_elapse() {
    let now = Local::now();

    assert!(is_stale(now - chrono::Duration::seconds(300), now, 300));
    assert!(!is_stale(now - chrono::Duration::seconds(299), now, 300));
}

#[test]
fn latest_activity_ignores_setup_as_agent_output() {
    let store = Store::open_memory().expect("store");
    let mut task = make_task("t-setup-act");
    task.created_at = Local::now() - chrono::Duration::seconds(200);
    store.insert_task(&task).expect("insert task");
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-setup-act".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(190),
            event_kind: EventKind::Setup,
            detail: "Cargo target seeded: /tmp/target from /cache in 12ms".to_string(),
            metadata: None,
        })
        .expect("insert setup");

    let activity = latest_activity(&store, &task).expect("activity");

    assert_eq!(activity.event_count, 0);
    assert!(!activity.has_agent_bytes);
    assert_eq!(activity.timestamp, task.created_at);
    assert!(activity.detail.is_none());
}

#[test]
fn latest_activity_counts_transcript_bytes_as_progress() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let mut task = make_task("t-bytes-act");
    task.created_at = Local::now() - chrono::Duration::seconds(200);
    store.insert_task(&task).expect("insert task");
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-bytes-act".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(190),
            event_kind: EventKind::Setup,
            detail: "Cargo target seeded".to_string(),
            metadata: None,
        })
        .expect("insert setup");
    std::fs::create_dir_all(paths::task_dir("t-bytes-act")).expect("task dir");
    let transcript = paths::transcript_path("t-bytes-act");
    std::fs::write(&transcript, "agent chunk\n").expect("bytes");
    // mtime newer than created_at so activity advances on bytes.
    let newer = std::time::SystemTime::now() - std::time::Duration::from_secs(10);
    std::fs::File::open(&transcript)
        .expect("open")
        .set_modified(newer)
        .expect("mtime");

    let activity = latest_activity(&store, &task).expect("activity");

    assert_eq!(activity.event_count, 0);
    assert!(activity.has_agent_bytes);
    assert!(activity.timestamp > task.created_at);
}

/// The agy failure this exists for. agy runs in print mode: stdout stays empty until a
/// turn completes, so transcript bytes prove nothing mid-turn and the reaper read
/// "no agent output since spawn" while agy was streaming from the model. Replay of
/// t-7fbbd0e7 (2026-08-08): agy's own log last grew at 13:39:26 and aid killed the task
/// at 13:41:56 — 150s, inside the 180s budget. Watching that log keeps it alive.
#[test]
fn latest_activity_counts_the_agent_s_own_log_as_progress() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let mut task = make_task("t-agy-log");
    task.created_at = Local::now() - chrono::Duration::seconds(200);
    store.insert_task(&task).expect("insert task");
    std::fs::create_dir_all(paths::task_dir("t-agy-log")).expect("task dir");

    // Nothing on stdout: transcript absent, exactly as print mode leaves it.
    let agent_log = paths::agent_log_path("t-agy-log");
    std::fs::write(&agent_log, "streamGenerateContent ... ResponseID: CM92\n").expect("bytes");
    let newer = std::time::SystemTime::now() - std::time::Duration::from_secs(150);
    std::fs::File::open(&agent_log).expect("open").set_modified(newer).expect("mtime");

    let activity = latest_activity(&store, &task).expect("activity");

    assert_eq!(activity.event_count, 0, "no parsed events — agy emits none");
    assert!(activity.has_agent_bytes, "the agent's own log is proof it is working");
    assert!(
        activity.timestamp > task.created_at,
        "and it must advance the clock, or the first-token budget still reaps it"
    );
}

/// Cross-audit finding: a leftover `agent.log` from an earlier attempt on the same task
/// id used to count as progress purely by existing, moving a spawn that produced nothing
/// off the 180s first-token budget onto 2x idle. Bytes older than the task say nothing
/// about it.
#[test]
fn latest_activity_ignores_agent_log_left_by_an_earlier_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let mut task = make_task("t-stale-log");
    task.created_at = Local::now() - chrono::Duration::seconds(200);
    store.insert_task(&task).expect("insert task");
    std::fs::create_dir_all(paths::task_dir("t-stale-log")).expect("task dir");

    let agent_log = paths::agent_log_path("t-stale-log");
    std::fs::write(&agent_log, "output from the previous attempt\n").expect("bytes");
    let before_the_task = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    std::fs::File::open(&agent_log).expect("open").set_modified(before_the_task).expect("mtime");

    let activity = latest_activity(&store, &task).expect("activity");

    assert!(
        !activity.has_agent_bytes,
        "a file older than the task is not evidence this run produced anything"
    );
    assert_eq!(activity.timestamp, task.created_at, "and it must not advance the clock");
}
