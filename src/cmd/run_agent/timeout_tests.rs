// Tests for foreground max-duration timeout policy.
// Covers active streams, idle deadline expiry, and default constant sharing.
// Deps: timeout helpers and shared config defaults.
use super::*;

#[test]
fn active_streaming_task_past_old_boundary_does_not_timeout() {
    let start = Instant::now();
    let old_boundary = Duration::from_millis(30);
    let now = start + old_boundary + Duration::from_millis(1);
    let last_activity = now - Duration::from_millis(5);

    assert!(!foreground_timeout_expired(
        start,
        last_activity,
        now,
        old_boundary,
        Duration::from_millis(10),
    ));
}

#[test]
fn idle_task_past_deadline_times_out() {
    let start = Instant::now();
    let max_duration = Duration::from_millis(30);
    let idle_timeout = Duration::from_millis(10);
    let now = start + max_duration + idle_timeout;

    assert!(foreground_timeout_expired(
        start,
        start,
        now,
        max_duration,
        idle_timeout,
    ));
}

#[test]
fn foreground_default_duration_uses_shared_config_constant() {
    assert_eq!(
        crate::timeout_policy::TimeoutPolicy::default().max_duration_mins(),
        crate::config::DEFAULT_MAX_TASK_DURATION_MINS
    );
}
