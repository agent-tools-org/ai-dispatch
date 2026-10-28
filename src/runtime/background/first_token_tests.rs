// first token tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

/// Buffered silent agents (grok) never hit MonitorState; the reaper is what
/// kills them. Zero agent output since spawn must use the first-token budget,
/// not 2× idle (the t-764b2a1d 1200s hang).
#[test]
fn reaps_silent_since_spawn_on_first_token_budget() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-silent0", TaskStatus::Running);
    // Past first-token (90s) but well under 2× idle (2×600 = 1200s).
    task.created_at = Local::now() - Duration::seconds(120);
    store.insert_task(&task).unwrap();
    let mut env = std::collections::HashMap::new();
    env.insert("AID_FIRST_TOKEN_TIMEOUT_SECS".to_string(), "90".to_string());
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(303),
        idle_timeout_secs: Some(600),
        env: Some(env),
        ..make_spec("t-silent0")
    })
    .unwrap();
    // No progress events and no agent bytes — activity falls back to created_at.

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 303).unwrap();

    assert_eq!(cleaned, vec!["t-silent0".to_string()]);
    assert_eq!(
        store.get_task("t-silent0").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-silent0").unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.detail.contains("no agent output since spawn")
                && e.detail.contains("first-token")),
        "detail={:?}",
        events.iter().map(|e| &e.detail).collect::<Vec<_>>()
    );
}

/// Real silent-since-spawn shape (t-764b2a1d): aid already wrote a Setup
/// event ("Cargo target seeded …") before the agent produced anything.
/// Setup is bookkeeping; with no agent bytes this is still first-token.
#[test]
fn reaps_buffered_setup_without_bytes_on_first_token_budget() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-setup0", TaskStatus::Running);
    task.created_at = Local::now() - Duration::seconds(120);
    store.insert_task(&task).unwrap();
    let mut env = std::collections::HashMap::new();
    env.insert("AID_FIRST_TOKEN_TIMEOUT_SECS".to_string(), "90".to_string());
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(305),
        idle_timeout_secs: Some(600),
        env: Some(env),
        ..make_spec("t-setup0")
    })
    .unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-setup0".to_string()),
            timestamp: Local::now() - Duration::seconds(115),
            event_kind: EventKind::Setup,
            detail: "Cargo target seeded: /tmp/target from /cache in 12ms".to_string(),
            metadata: None,
        })
        .unwrap();
    // No transcript/log bytes — buffered agent has produced nothing.

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 305).unwrap();

    assert_eq!(cleaned, vec!["t-setup0".to_string()]);
    assert_eq!(
        store.get_task("t-setup0").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-setup0").unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.detail.contains("no agent output since spawn")
                && e.detail.contains("first-token")),
        "setup without bytes must still first-token; detail={:?}",
        events.iter().map(|e| &e.detail).collect::<Vec<_>>()
    );
}

/// Buffered path with real agent bytes: past first-token but under 2× idle
/// must stay alive. Events cannot answer this — watch_buffered only inserts
/// the completion event after exit; bytes are the progress signal.
#[test]
fn keeps_buffered_with_agent_bytes_within_idle_budget() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-bytes1", TaskStatus::Running);
    // Past first-token (90s) but well under 2× idle (2×600 = 1200s).
    task.created_at = Local::now() - Duration::seconds(120);
    store.insert_task(&task).unwrap();
    let mut env = std::collections::HashMap::new();
    env.insert("AID_FIRST_TOKEN_TIMEOUT_SECS".to_string(), "90".to_string());
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(306),
        idle_timeout_secs: Some(600),
        env: Some(env),
        ..make_spec("t-bytes1")
    })
    .unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-bytes1".to_string()),
            timestamp: Local::now() - Duration::seconds(115),
            event_kind: EventKind::Setup,
            detail: "Cargo target seeded: /tmp/target from /cache in 12ms".to_string(),
            metadata: None,
        })
        .unwrap();
    // Simulate watch_buffered having observed stdout bytes — age the file so
    // activity is past first-token; only the 2× idle budget keeps it alive.
    std::fs::create_dir_all(paths::task_dir("t-bytes1")).unwrap();
    let transcript = paths::transcript_path("t-bytes1");
    std::fs::write(
        &transcript,
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"working\"}\n",
    )
    .unwrap();
    let aged = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    std::fs::File::open(&transcript)
        .unwrap()
        .set_modified(aged)
        .unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 306).unwrap();

    assert!(
        cleaned.is_empty(),
        "bytes-seen buffered task must use 2× idle, not first-token; cleaned={cleaned:?}"
    );
    assert_eq!(
        store.get_task("t-bytes1").unwrap().unwrap().status,
        TaskStatus::Running
    );
}

#[test]
fn keeps_silent_since_spawn_within_first_token_budget() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-quiet", TaskStatus::Running);
    task.created_at = Local::now() - Duration::seconds(30);
    store.insert_task(&task).unwrap();
    let mut env = std::collections::HashMap::new();
    env.insert("AID_FIRST_TOKEN_TIMEOUT_SECS".to_string(), "90".to_string());
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(304),
        idle_timeout_secs: Some(600),
        env: Some(env),
        ..make_spec("t-quiet")
    })
    .unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 304).unwrap();

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-quiet").unwrap().unwrap().status,
        TaskStatus::Running
    );
}
