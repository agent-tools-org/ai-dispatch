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

fn aid_home() -> (tempfile::TempDir, paths::AidHomeGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    (temp, home)
}

fn write_agent_log(id: &str, body: &str) {
    std::fs::create_dir_all(paths::task_dir(id)).expect("task dir");
    std::fs::write(paths::agent_log_path(id), body).expect("write log");
}

fn age_log(id: &str, age: Duration) {
    let stale = std::time::SystemTime::now() - age;
    std::fs::File::open(paths::agent_log_path(id))
        .expect("open")
        .set_modified(stale)
        .expect("mtime");
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
    // t-54c4560a shape: PTY silent, progress stale, no recent agent.log → reap.
    let (_temp, _home) = aid_home();
    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);
    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-dead"),
        "buffered agent with no log growth must be reaped"
    );
}

#[test]
fn idle_hang_is_inert_for_buffered_with_log_growth_in_window() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-buf-idle-live", "tool call in flight...\n");
    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);
    assert!(
        !state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-live"),
        "buffered agent whose log grew inside the idle window must survive"
    );
}

#[test]
fn idle_hang_fires_for_buffered_when_log_is_stale() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-buf-idle-stale", "wrote once then hung\n");
    age_log("t-buf-idle-stale", Duration::from_secs(700));
    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);
    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-stale"),
        "stale agent.log must not keep a hung buffered agent alive"
    );
}

#[test]
fn idle_hang_streaming_ignores_agent_log() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-stream-log", "unrelated log growth\n");
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);
    assert!(
        state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-log"),
        "streaming agents must not gain idle immunity from agent.log"
    );
}

#[test]
fn maybe_handle_idle_skips_ladder_when_buffered_log_grows() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-buf-ladder-live", "streamGenerateContent in flight\n");
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-buf-ladder-live");
    let mut state = MonitorState::with_policy(false, None, short_ladder_policy());
    let stale = Instant::now() - Duration::from_secs(25);
    state.last_progress_time = stale;
    state.maybe_handle_idle(&store, &task_id, true).expect("idle ladder");
    assert_eq!(state.last_progress_time, stale, "ladder must not reset hang clock");
    assert!(!state.idle_warned && !state.idle_nudged);
    let events = store.get_events(task_id.as_str()).expect("events");
    assert!(
        events.iter().all(|e| e.detail != "idle warn" && e.detail != "Auto-nudge sent"),
        "ladder must not fire while buffered log is alive: {events:?}"
    );
}

#[test]
fn repeated_ladder_ticks_with_log_growth_still_reap_after_idle() {
    // Round-2: mark_progress from buffered growth reset the hang clock every
    // tick. Multi-tick growth must leave last_progress_time alone.
    let (_temp, _home) = aid_home();
    write_agent_log("t-buf-multi-tick", "start\n");
    let agent_log = paths::agent_log_path("t-buf-multi-tick");
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-buf-multi-tick");
    let mut state = MonitorState::with_policy(false, None, short_ladder_policy());
    let stale_progress = Instant::now() - Duration::from_secs(601);
    state.last_progress_time = stale_progress;

    for i in 0..5 {
        std::fs::write(&agent_log, format!("tool call {i}\n")).expect("write log");
        state.maybe_handle_idle(&store, &task_id, true).expect("idle ladder");
        assert!(
            !state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-multi-tick"),
            "growing log must suppress hang while alive"
        );
    }
    assert_eq!(state.last_progress_time, stale_progress);
    assert!(!state.idle_warned && !state.idle_nudged);

    age_log("t-buf-multi-tick", Duration::from_secs(700));
    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-multi-tick"),
        "after log freezes, prior ladder ticks must not immunise the agent"
    );
}

#[test]
fn maybe_handle_idle_warns_when_buffered_log_is_stale() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-buf-ladder-stale", "wrote once then hung\n");
    age_log("t-buf-ladder-stale", Duration::from_secs(60));
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-buf-ladder-stale");
    let mut state = MonitorState::with_policy(false, None, short_ladder_policy());
    state.last_progress_time = Instant::now() - Duration::from_secs(15);
    state.maybe_handle_idle(&store, &task_id, true).expect("idle ladder");
    assert!(state.idle_warned, "stale buffered log must still warn");
    let events = store.get_events(task_id.as_str()).expect("events");
    assert!(
        events.iter().any(|e| e.detail == "idle warn"),
        "expected idle warn event: {events:?}"
    );
}

#[test]
fn maybe_handle_idle_streaming_ignores_agent_log() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-stream-ladder", "unrelated log growth\n");
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-stream-ladder");
    let mut state = MonitorState::with_policy(true, None, short_ladder_policy());
    state.last_progress_time = Instant::now() - Duration::from_secs(15);
    state.maybe_handle_idle(&store, &task_id, true).expect("idle ladder");
    assert!(
        state.idle_warned,
        "streaming ladder must stay on the progress clock even if a log grows"
    );
}
