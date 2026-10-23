// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn maybe_handle_idle_warns_when_buffered_log_is_stale() {
    let (_temp, _home) = aid_home();
    write_agent_log("t-buf-ladder-stale", "wrote once then hung\n");
    age_log("t-buf-ladder-stale", Duration::from_secs(60));
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-buf-ladder-stale", AgentKind::Antigravity);
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
    let (_temp, _home) = aid_home();
    write_agent_log("t-stream-ladder", "unrelated log growth\n");
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-stream-ladder", AgentKind::Antigravity);
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

#[test]
fn idle_ladder_never_queues_for_noninteractive_agent() {
    let (_temp, _home) = aid_home();
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = running_task(&store, "t-nudge-noninteractive", AgentKind::Antigravity);
    let agent = crate::agent::get_agent(AgentKind::Antigravity);
    let mut state = MonitorState::with_policy(false, None, short_ladder_policy());
    state.last_progress_time = Instant::now() - Duration::from_secs(25);

    assert!(!agent.accepts_interactive_input());
    state
        .maybe_handle_idle(
            &store,
            &task_id,
            agent.accepts_interactive_input() && agent.accepts_idle_nudge(),
        )
        .expect("idle ladder");

    assert!(
        store
            .list_messages_for_task(task_id.as_str())
            .expect("messages")
            .is_empty()
    );
    assert!(!state.idle_nudged);
}
