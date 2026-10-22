// Marker readback distinguishes provider evidence from the default plan cooldown.
// Uses real marker entry points and isolated state without an operator Codex home.
use super::*;
use crate::support::paths::AidHomeGuard;

#[test]
fn dateless_provider_refusal_writes_manual_hold_while_default_keeps_its_clock() {
    let home = tempfile::tempdir().unwrap();
    let _home = AidHomeGuard::set(home.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let route = Some("codex-provider:X");
    let message = "You have hit your usage limit";
    for name in [None, route] {
        mark_rate_limited(&AgentKind::Codex, name, message);
        let info = get_rate_limit_info(&AgentKind::Codex, name).unwrap();
        assert_eq!(info.needs_human, name.is_some());
        assert_eq!(info.recovery_at.is_some(), name.is_none());
        assert!(info.marker.contains("signature_agent: codex"));
    }
    mark_group_rate_limited(&AgentKind::Codex, route, "provider-quota", message);
    let marker = fs::read_to_string(group_marker_path(
        &AgentKind::Codex,
        route,
        "provider-quota",
    ))
    .unwrap();
    assert_eq!(marker_field(&marker, "hold: ").as_deref(), Some("manual"));
    assert!(marker_field(&marker, "recovery_at: ").is_none());
    assert_eq!(
        availability_for_group(&AgentKind::Codex, route, "provider-quota").status,
        RouteStatus::Held
    );
}

#[test]
fn provider_stated_reset_and_capacity_keep_clocks_and_generic_refusals_stay_transient() {
    let home = tempfile::tempdir().unwrap();
    let _home = AidHomeGuard::set(home.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let route = Some("codex-provider:X");
    let default_marker = marker_path(&AgentKind::Codex, None);
    fs::write(&default_marker, "hold: manual\nmessage: default plan\n").unwrap();
    for (message, dated) in [
        (
            "You have hit your usage limit; try again at 2099-01-02T03:04:05Z",
            true,
        ),
        (
            "Selected model is at capacity. Please try a different model.",
            true,
        ),
        ("HTTP 429", false),
    ] {
        mark_rate_limited(&AgentKind::Codex, route, message);
        let info = get_rate_limit_info(&AgentKind::Codex, route).unwrap();
        assert!(!info.needs_human, "{message}");
        assert_eq!(info.recovery_at.is_some(), dated, "{message}");
        assert!(clear_rate_limit(&AgentKind::Codex, route));
        assert_eq!(fs::read_to_string(&default_marker).unwrap(),
            "hold: manual\nmessage: default plan\n");
    }
}
