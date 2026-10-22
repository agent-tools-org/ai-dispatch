// Idle policy and inbound echo tests.
// Exercises the parent policy with bounded timing and echo budgets.
use super::*;

fn detector() -> IdleDetector {
    IdleDetector {
        warn_after: Duration::from_secs(10),
        nudge_after: Duration::from_secs(20),
        escalate_after: Duration::from_secs(30),
    }
}

#[test]
fn idle_thresholds_follow_expected_transitions() {
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(9),
            MonitorTaskStatus::Running,
            false,
            true,
        ),
        IdleAction::None
    );
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(10),
            MonitorTaskStatus::Running,
            false,
            true,
        ),
        IdleAction::WarnEvent
    );
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(20),
            MonitorTaskStatus::Running,
            false,
            true,
        ),
        IdleAction::SendNudge(default_nudge_message())
    );
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(30),
            MonitorTaskStatus::Running,
            true,
            true,
        ),
        IdleAction::Escalate
    );
}

#[test]
fn non_running_states_never_emit_idle_actions() {
    for status in [
        MonitorTaskStatus::AwaitingInput,
        MonitorTaskStatus::Stalled,
        MonitorTaskStatus::Inactive,
    ] {
        assert_eq!(
            detector().tick(Instant::now() - Duration::from_secs(60), status, true, true),
            IdleAction::None
        );
    }
}

#[test]
fn tick_skips_nudge_for_agents_that_do_not_accept_nudges() {
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(20),
            MonitorTaskStatus::Running,
            false,
            false,
        ),
        IdleAction::None
    );
}

#[test]
fn tick_escalates_without_prior_nudge_when_agent_does_not_accept_nudges() {
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(30),
            MonitorTaskStatus::Running,
            false,
            false,
        ),
        IdleAction::Escalate
    );
}

#[test]
fn tick_still_sends_nudge_for_agents_that_accept_nudges() {
    assert_eq!(
        detector().tick(
            Instant::now() - Duration::from_secs(20),
            MonitorTaskStatus::Running,
            false,
            true,
        ),
        IdleAction::SendNudge(default_nudge_message())
    );
}

#[test]
fn take_inbound_echo_suppresses_terminal_echo_and_immediate_duplicate() {
    let mut pending = Vec::new();
    register_inbound_echo(&mut pending, default_nudge_message());
    register_inbound_echo(&mut pending, "other".to_string());
    assert!(take_inbound_echo(
        &mut pending,
        "  Task appears idle. Status update please?  "
    ));
    assert!(take_inbound_echo(&mut pending, &default_nudge_message()));
    assert!(!take_inbound_echo(&mut pending, &default_nudge_message()));
    assert!(take_inbound_echo(&mut pending, "other"));
    assert!(take_inbound_echo(&mut pending, "other"));
    assert!(pending.is_empty());
}

#[test]
fn inbound_echo_suppression_covers_delayed_terminal_echo() {
    let registered_at = Instant::now();
    let mut pending = Vec::new();
    register_inbound_echo(&mut pending, default_nudge_message());

    assert!(take_inbound_echo_at(
        &mut pending,
        &default_nudge_message(),
        registered_at + Duration::from_secs(6),
    ));
}

#[test]
fn inbound_echo_suppression_expires_after_bounded_window() {
    let registered_at = Instant::now();
    let mut pending = Vec::new();
    register_inbound_echo(&mut pending, default_nudge_message());

    assert!(!take_inbound_echo_at(
        &mut pending,
        &default_nudge_message(),
        registered_at + INBOUND_ECHO_WINDOW + Duration::from_millis(1),
    ));
    assert!(pending.is_empty());
}

#[test]
fn inbound_echo_suppression_has_a_finite_pending_entry_cap() {
    let mut pending = Vec::new();
    for index in 0..=MAX_PENDING_INBOUND_ECHOES {
        register_inbound_echo(&mut pending, format!("message-{index}"));
    }
    assert_eq!(pending.len(), MAX_PENDING_INBOUND_ECHOES);
}

#[test]
fn third_matching_echo_inside_window_is_real_output() {
    let mut pending = Vec::new();
    register_inbound_echo(&mut pending, default_nudge_message());
    assert!(take_inbound_echo(&mut pending, &default_nudge_message()));
    assert!(take_inbound_echo(&mut pending, &default_nudge_message()));
    assert!(!take_inbound_echo(&mut pending, &default_nudge_message()));
}
