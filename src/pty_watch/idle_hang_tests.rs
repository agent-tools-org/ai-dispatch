// Tests for PTY idle hang detection with buffered-agent log liveness.
// Deps: MonitorState, paths::agent_log_path, AidHomeGuard.

use super::MonitorState;
use crate::paths;
use crate::store::Store;
use crate::timeout_policy::{NudgeLadder, TimeoutPolicy};
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn short_ladder_policy() -> TimeoutPolicy {
    TimeoutPolicy {
        idle: Duration::from_secs(600),
        first_token: Duration::from_secs(180),
        nudge_ladder: NudgeLadder {
            warn: Duration::from_secs(10),
            nudge: Duration::from_secs(20),
            escalate: Duration::from_secs(30),
        },
        max_duration: Duration::from_secs(3600),
        hard_cap: Duration::from_secs(86400),
    }
}

fn running_task(store: &Store, id: &str) -> TaskId {
    let task = Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Gemini,
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
    store.insert_task(&task).expect("insert task");
    task.id
}

#[test]
fn idle_hang_fires_for_streaming_when_progress_stale() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-idle"));
}

#[test]
fn idle_hang_is_inert_for_streaming_with_fresh_progress() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);

    assert!(!state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-fresh"));
}

#[test]
fn idle_hang_fires_for_buffered_without_log_growth() {
    // Replay of t-54c4560a shape: PTY silent, progress clock stale, no recent
    // agent.log write → idle watchdog must still reap.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");

    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-dead"),
        "buffered agent with no log growth must be reaped"
    );
}

#[test]
fn idle_hang_is_inert_for_buffered_with_log_growth_in_window() {
    // grok/agy write agent.log while the PTY stays silent. A log written
    // inside the idle window is growth, not mere existence — keep alive.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-idle-live")).expect("task dir");

    let agent_log = paths::agent_log_path("t-buf-idle-live");
    std::fs::write(&agent_log, "tool call in flight...\n").expect("write log");

    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        !state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-live"),
        "buffered agent whose log grew inside the idle window must survive"
    );
}

#[test]
fn idle_hang_fires_for_buffered_when_log_is_stale() {
    // A log that merely exists from earlier in the run is not proof of life.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-idle-stale")).expect("task dir");

    let agent_log = paths::agent_log_path("t-buf-idle-stale");
    std::fs::write(&agent_log, "wrote once then hung\n").expect("write log");
    let stale = std::time::SystemTime::now() - Duration::from_secs(700);
    std::fs::File::open(&agent_log)
        .expect("open")
        .set_modified(stale)
        .expect("mtime");

    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-stale"),
        "stale agent.log must not keep a hung buffered agent alive"
    );
}

#[test]
fn idle_hang_streaming_ignores_agent_log() {
    // Streaming idle semantics stay progress-clock only even if a log exists.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-stream-log")).expect("task dir");
    std::fs::write(
        paths::agent_log_path("t-stream-log"),
        "unrelated log growth\n",
    )
    .expect("write log");

    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-log"),
        "streaming agents must not gain idle immunity from agent.log"
    );
}

#[test]
fn maybe_handle_idle_skips_ladder_when_buffered_log_grows() {
    // Live failure shape (t-54c4560a): PTY silent, progress stale, agent.log
    // still growing — warn/nudge/escalate must stay quiet via the shared helper.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-ladder-live")).expect("task dir");
    std::fs::write(
        paths::agent_log_path("t-buf-ladder-live"),
        "streamGenerateContent in flight\n",
    )
    .expect("write log");

    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-buf-ladder-live");
    let mut state = MonitorState::with_policy(false, None, short_ladder_policy());
    state.last_progress_time = Instant::now() - Duration::from_secs(25);

    state
        .maybe_handle_idle(&store, &task_id, true)
        .expect("idle ladder");

    assert!(!state.idle_warned, "growing agent.log must suppress idle warn");
    assert!(!state.idle_nudged, "growing agent.log must suppress auto-nudge");
    let events = store.get_events(task_id.as_str()).expect("events");
    assert!(
        events.iter().all(|e| e.detail != "idle warn" && e.detail != "Auto-nudge sent"),
        "ladder must not fire while buffered log is alive: {events:?}"
    );
}

#[test]
fn maybe_handle_idle_warns_when_buffered_log_is_stale() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-ladder-stale")).expect("task dir");
    let agent_log = paths::agent_log_path("t-buf-ladder-stale");
    std::fs::write(&agent_log, "wrote once then hung\n").expect("write log");
    let stale = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::File::open(&agent_log)
        .expect("open")
        .set_modified(stale)
        .expect("mtime");

    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-buf-ladder-stale");
    let mut state = MonitorState::with_policy(false, None, short_ladder_policy());
    state.last_progress_time = Instant::now() - Duration::from_secs(15);

    state
        .maybe_handle_idle(&store, &task_id, true)
        .expect("idle ladder");

    assert!(state.idle_warned, "stale buffered log must still warn");
    let events = store.get_events(task_id.as_str()).expect("events");
    assert!(
        events.iter().any(|e| e.detail == "idle warn"),
        "expected idle warn event: {events:?}"
    );
}

#[test]
fn maybe_handle_idle_streaming_ignores_agent_log() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-stream-ladder")).expect("task dir");
    std::fs::write(
        paths::agent_log_path("t-stream-ladder"),
        "unrelated log growth\n",
    )
    .expect("write log");

    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-stream-ladder");
    let mut state = MonitorState::with_policy(true, None, short_ladder_policy());
    state.last_progress_time = Instant::now() - Duration::from_secs(15);

    state
        .maybe_handle_idle(&store, &task_id, true)
        .expect("idle ladder");

    assert!(
        state.idle_warned,
        "streaming ladder must stay on the progress clock even if a log grows"
    );
}
