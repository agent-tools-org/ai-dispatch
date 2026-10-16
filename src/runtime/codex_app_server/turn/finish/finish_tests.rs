// End-of-turn flushing, stderr, stop grace, and late-steer receipt regressions.
// Uses a fake server that explicitly persists a marker after input EOF.

use crate::runtime::codex_app_server::*;
#[path = "../fixture.rs"]
mod fixture;
use fixture::Fixture;

#[test]
fn late_final_items_and_session_flush_survive_turn_completion() {
    for mode in ["late-final", "flush"] {
        let fixture = Fixture::new(mode);
        fixture.dispatch();
        assert_eq!(
            fixture
                .store
                .get_task(fixture.task.as_str())
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Done
        );
        assert_eq!(std::fs::read_to_string(&fixture.output).unwrap(), "STEERED");
        assert!(fixture.home.path().join("flushed").exists());
    }
}

#[test]
fn failure_preserves_capped_stderr_in_events_and_log() {
    let fixture = Fixture::new("stderr-turn");
    fixture.dispatch();
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("synthetic turn diagnostic"))
    );
    assert!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .contains("synthetic turn diagnostic")
    );
}

#[test]
fn approval_and_permission_requests_decline_without_ending_the_turn() {
    let fixture = Fixture::new("approvals");
    fixture.dispatch();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Done
    );
}

#[test]
fn slow_interrupt_acknowledgement_fits_inside_stop_grace() {
    let fixture = Fixture::new("slow-interrupt");
    let actor = fixture.actor_mode("timed-stop");
    fixture.dispatch();
    actor.join().unwrap();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Stopped
    );
}

#[test]
fn steer_accepted_after_ten_seconds_is_still_confirmed() {
    let fixture = Fixture::new("late-steer");
    let actor = fixture.actor(false);
    fixture.dispatch();
    actor.join().unwrap();
    let messages = fixture
        .store
        .list_messages_for_task(fixture.task.as_str())
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].delivered_at.is_some());
    assert!(
        receipt(&fixture.store, fixture.task.as_str(), messages[0].id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn requests_requiring_credentials_fail_clearly_without_fallback() {
    let fixture = Fixture::new("unknown-request");
    fixture.dispatch();
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("Unsupported Codex server request"))
    );
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn malformed_trailing_protocol_is_not_hidden_by_terminal_success() {
    let fixture = Fixture::new("corrupt-final");
    fixture.dispatch();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn shutdown_failure_does_not_override_the_process_error_with_terminal_success() {
    let fixture = Fixture::new("exit-final");
    fixture.dispatch();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}
