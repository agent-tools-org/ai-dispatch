// Existing stale-marker and group clearing regressions.
// Deps: parent rate-limit facade and isolated aid home.

use super::*;
use crate::types::AgentKind;

/// A marker this run wrote must outlive this run's success. Clearing it is
/// how an outage recorded by `record_quota_exhaustion` was handed straight
/// back to routing by `handle_done_postprocess`.
#[test]
fn a_marker_written_during_the_run_is_not_cleared_by_success() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    clear_rate_limit(&AgentKind::Qwen, None);

    let task_start = Local::now() - chrono::Duration::minutes(5);
    mark_rate_limited(
        &AgentKind::Qwen,
        None,
        "Your token-plan 5-hour quota has been exhausted.",
    );

    assert!(!clear_rate_limit_if_stale(
        &AgentKind::Qwen,
        None,
        task_start
    ));
    assert!(is_rate_limited(&AgentKind::Qwen, None));
}

/// A marker left by an earlier run is stale and a fresh success clears it,
/// exactly as before this change.
#[test]
fn a_marker_from_an_earlier_run_is_still_cleared() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    clear_rate_limit(&AgentKind::Qwen, None);

    mark_rate_limited(
        &AgentKind::Qwen,
        None,
        "Your token-plan 5-hour quota has been exhausted.",
    );
    let task_start = Local::now() + chrono::Duration::minutes(5);

    assert!(clear_rate_limit_if_stale(
        &AgentKind::Qwen,
        None,
        task_start
    ));
    assert!(!is_rate_limited(&AgentKind::Qwen, None));
}

#[test]
fn clear_group_rate_limit_if_stale_clears_only_matching_group() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let agent = AgentKind::Antigravity;
    clear_all_rate_limits_for_agent(&agent, None);

    mark_rate_limited(&agent, None, "Agent rate limit");
    mark_group_rate_limited(&agent, None, "gemini", "Gemini quota exhausted");
    mark_group_rate_limited(&agent, None, "claude", "Claude quota exhausted");

    let task_start = Local::now() + chrono::Duration::minutes(5);

    let cleared = clear_rate_limit_for_model_if_stale(
        &agent,
        None,
        Some("gemini-3.6-flash-high"),
        task_start,
    );
    assert!(cleared, "gemini group marker should be cleared on success");

    assert!(
        !is_rate_limited(&agent, None),
        "agent-level marker must be cleared on model success"
    );
    assert!(
        !is_group_rate_limited(&agent, None, "gemini"),
        "gemini group must no longer be limited"
    );
    assert!(
        is_group_rate_limited(&agent, None, "claude"),
        "claude group must remain limited"
    );
}

#[test]
fn clear_rate_limit_does_not_clear_group_markers() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let agent = AgentKind::Antigravity;
    clear_all_rate_limits_for_agent(&agent, None);

    mark_rate_limited(&agent, None, "Agent level limit");
    mark_group_rate_limited(&agent, None, "gemini", "Gemini quota exhausted");

    assert!(clear_rate_limit(&agent, None));
    assert!(
        !is_rate_limited(&agent, None),
        "agent-level marker must be removed"
    );
    assert!(
        is_group_rate_limited(&agent, None, "gemini"),
        "group marker must NOT be removed by clear_rate_limit"
    );
}

#[test]
fn clear_all_rate_limits_clears_agent_and_all_groups() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let agent = AgentKind::Antigravity;
    clear_all_rate_limits_for_agent(&agent, None);

    mark_rate_limited(&agent, None, "Agent limit");
    mark_group_rate_limited(&agent, None, "gemini", "Gemini limit");
    mark_group_rate_limited(&agent, None, "claude", "Claude limit");

    assert!(clear_all_rate_limits_for_agent(&agent, None));
    assert!(!is_rate_limited(&agent, None));
    assert!(!is_group_rate_limited(&agent, None, "gemini"));
    assert!(!is_group_rate_limited(&agent, None, "claude"));
}
