// Regression coverage for descendant activity, first-output parity, and hung retry evidence.
// The fake server starts real silent child work without invoking a model.

use super::*;
use crate::runtime::codex_app_server::fixture::Fixture;

fn policy(idle_ms: u64) -> crate::config::timeout_policy::TimeoutPolicy {
    let mut policy = crate::config::timeout_policy::TimeoutPolicy::default();
    policy.idle = Duration::from_millis(idle_ms);
    policy.first_token = Duration::from_millis(100);
    policy
}

#[test]
fn descendant_activity_only_extends_idle_after_first_output() {
    let old = SystemTime::now() - Duration::from_secs(5);
    assert!(!expired(
        old,
        Some(SystemTime::now()),
        true,
        Duration::from_secs(1)
    ));
    assert!(expired(
        old,
        Some(SystemTime::now()),
        false,
        Duration::from_secs(1)
    ));
}

#[test]
fn quiet_cpu_child_survives_the_notification_idle_limit() {
    let fixture = Fixture::new("busy");
    fixture.dispatch_with_policy(policy(500));
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Done
    );
    assert!(
        monitor::hung_context(&fixture.store.get_events(fixture.task.as_str()).unwrap()).is_none()
    );
}

#[test]
fn silence_before_first_item_uses_piped_exec_idle_window() {
    let fixture = Fixture::new("first-output");
    fixture.dispatch_with_policy(policy(1500));
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
fn idle_timeout_records_the_metadata_used_by_hung_auto_retry() {
    let fixture = Fixture::new("hung");
    fixture.dispatch_with_policy(policy(500));
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    let context =
        monitor::hung_context(&events).expect("eligible for the existing hung recovery path");
    assert!(!context.transient);
    assert!(context.event_count > 0);
    assert!(events.iter().any(|event| event.detail == "hung_detected"));
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
