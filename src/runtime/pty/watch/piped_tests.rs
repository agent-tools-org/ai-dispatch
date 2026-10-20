// Shared liveness policy regression tests for piped Claude workers.
// Exercises raw first-token activity, meaningful progress, and idle escalation.
use super::*;

fn fixture() -> (
    tempfile::TempDir,
    crate::support::paths::AidHomeGuard,
    Arc<Store>,
    TaskId,
) {
    let home = tempfile::tempdir().unwrap();
    let guard = crate::support::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task = TaskId("t-piped-clock".into());
    store.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES (?1,'claude','p','running',?2)",
        rusqlite::params![task.as_str(), chrono::Local::now().to_rfc3339()],
    ).unwrap();
    (home, guard, store, task)
}

#[test]
fn first_token_gate_uses_raw_reads_and_separate_timeout() {
    let (_home, _guard, store, task) = fixture();
    let mut watchdog = Watchdog::new(crate::config::timeout_policy::TimeoutPolicy::default());
    watchdog.state.last_raw_chunk_time =
        Instant::now() - watchdog.policy.first_token - Duration::from_secs(1);
    assert!(watchdog.tick(&store, &task, None).unwrap());
    assert!(watchdog.first_token());
    assert_eq!(watchdog.timeout(), watchdog.policy.first_token);
    watchdog.raw.lock().unwrap().raw = Some(Instant::now());
    assert!(!watchdog.tick(&store, &task, None).unwrap());
    watchdog.observe("meaningful output", 2);
    watchdog.raw.lock().unwrap().raw = None;
    watchdog.state.last_raw_chunk_time =
        Instant::now() - watchdog.policy.first_token - Duration::from_secs(1);
    assert!(!watchdog.tick(&store, &task, None).unwrap());
}

#[test]
fn replay_does_not_reset_idle_and_silent_process_is_hung() {
    let (_home, _guard, store, task) = fixture();
    let mut watchdog = Watchdog::new(crate::config::timeout_policy::TimeoutPolicy::default());
    watchdog.observe("working", 2);
    watchdog.state.last_progress_time =
        Instant::now() - watchdog.policy.idle - Duration::from_secs(1);
    let before = watchdog.state.last_progress_time;
    watchdog.observe(r#"{"type":"user","isReplay":true}"#, 2);
    assert_eq!(watchdog.state.last_progress_time, before);
    assert!(watchdog.tick(&store, &task, None).unwrap());
    assert!(!watchdog.first_token());
    assert_eq!(watchdog.timeout(), watchdog.policy.idle);
}

#[test]
fn idle_warn_and_escalation_match_pty_without_nudging_claude() {
    let (_home, _guard, store, task) = fixture();
    let mut watchdog = Watchdog::new(crate::config::timeout_policy::TimeoutPolicy::default());
    watchdog.policy.idle = Duration::from_secs(1000);
    watchdog.observe("working", 2);
    watchdog.state.last_progress_time = Instant::now() - watchdog.policy.nudge_ladder.warn;
    assert!(!watchdog.tick(&store, &task, None).unwrap());
    assert!(
        store
            .get_events(task.as_str())
            .unwrap()
            .iter()
            .any(|event| event.detail == "idle warn")
    );
    watchdog.state.last_progress_time = Instant::now() - watchdog.policy.nudge_ladder.escalate;
    assert!(!watchdog.tick(&store, &task, None).unwrap());
    assert_eq!(
        store.get_task(task.as_str()).unwrap().unwrap().status,
        TaskStatus::Stalled
    );
    assert!(
        store
            .list_messages_for_task(task.as_str())
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn partial_raw_reads_refresh_first_token_clock() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut watchdog = Watchdog::new(crate::config::timeout_policy::TimeoutPolicy::default());
    watchdog.state.last_raw_chunk_time = Instant::now() - Duration::from_secs(200);
    let (read, mut write) = tokio::io::duplex(8);
    write.write_all(b"partial").await.unwrap();
    let mut reader = Reader::new(read, Some(&watchdog), None, false);
    let mut bytes = [0; 7];
    reader.read_exact(&mut bytes).await.unwrap();
    assert!(watchdog.raw.lock().unwrap().raw.is_some());
}

#[test]
fn childless_activity_probe_is_throttled_across_output_lines() {
    let mut watchdog = Watchdog::new(crate::config::timeout_policy::TimeoutPolicy::default());
    watchdog.state.received_raw_bytes = true;
    let task = TaskId("t-probe-cadence".into());
    let start = Instant::now();
    let mut calls = 0;
    for millis in [0, 0, 100, 200, 499, 500, 500, 600, 999, 1000] {
        watchdog.observe("another output line", 2);
        watchdog.refresh_descendant_activity(
            &task,
            Some(123),
            start + Duration::from_millis(millis),
            |_, _, _| {
                calls += 1;
                None
            },
        );
    }
    assert_eq!(calls, 3);
    watchdog.policy.idle = Duration::from_millis(400);
    watchdog.refresh_descendant_activity(
        &task,
        Some(123),
        start + Duration::from_millis(1099),
        |_, _, _| {
            calls += 1;
            None
        },
    );
    assert_eq!(calls, 3);
    watchdog.refresh_descendant_activity(
        &task,
        Some(123),
        start + Duration::from_millis(1100),
        |_, _, _| {
            calls += 1;
            None
        },
    );
    assert_eq!(calls, 4);
}
