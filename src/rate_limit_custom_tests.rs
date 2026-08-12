// Custom-agent rate-limit marker isolation tests.
// Verifies two customs do not share a hold, and built-in paths stay unchanged.

use super::*;
use crate::paths::{self, AidHomeGuard};
use crate::types::AgentKind;

#[test]
fn two_custom_agents_do_not_share_a_hold() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).unwrap();

    let stated = crate::rate_limit::test_future_recovery_time();
    mark_rate_limited(
        &AgentKind::Custom,
        Some("auditor"),
        &format!("try again at {stated}."),
    );

    assert!(is_rate_limited(&AgentKind::Custom, Some("auditor")));
    assert!(!is_rate_limited(&AgentKind::Custom, Some("reviewer")));
    assert!(!is_rate_limited(&AgentKind::Custom, None));
    assert!(paths::aid_dir().join("rate-limit-auditor").exists());
    assert!(!paths::aid_dir().join("rate-limit-custom").exists());
    assert!(!paths::aid_dir().join("rate-limit-reviewer").exists());

    assert!(dispatch_blocking_hold(&AgentKind::Custom, Some("auditor")).is_some());
    assert!(dispatch_blocking_hold(&AgentKind::Custom, Some("reviewer")).is_none());

    assert!(clear_rate_limit(&AgentKind::Custom, Some("auditor")));
    assert!(!is_rate_limited(&AgentKind::Custom, Some("auditor")));
}

#[test]
fn builtin_marker_slug_unchanged() {
    assert_eq!(marker_slug(&AgentKind::Codex, None), "codex");
    assert_eq!(marker_slug(&AgentKind::Codex, Some("ignored")), "codex");
    assert_eq!(marker_slug(&AgentKind::Custom, Some("glm5")), "glm5");
    assert_eq!(marker_slug(&AgentKind::Custom, None), "custom");
}

#[test]
fn resolve_agent_maps_custom_names() {
    assert_eq!(resolve_agent("codex"), (AgentKind::Codex, None));
    assert_eq!(resolve_agent("glm5"), (AgentKind::Custom, Some("glm5")));
}
