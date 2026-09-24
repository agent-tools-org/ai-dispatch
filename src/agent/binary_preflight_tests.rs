// Pins the shared route predicate used by the dispatch guards and preflight.
// Deps: binary guards, CursorBinaryGuard.
use super::{
    cursor::CursorBinaryGuard, ensure_agent_binary_available_with,
    ensure_resolved_binary_available_with,
};
use crate::types::AgentKind;

#[test]
fn route_guard_rejects_missing_kilo_binary() {
    assert!(ensure_agent_binary_available_with(AgentKind::Kilo, "kilo", |_| false).is_err());
}

#[test]
fn route_guard_rejects_missing_mimocode_binary() {
    assert!(ensure_agent_binary_available_with(AgentKind::MiMoCode, "mimocode", |_| false).is_err());
}

#[test]
fn route_guard_checks_the_cursor_program_the_adapter_resolved() {
    let _guard = CursorBinaryGuard::set("cursor-agent");
    let only_alias = |name: &str| name == "cursor-agent";
    assert!(ensure_agent_binary_available_with(AgentKind::Cursor, "cursor", only_alias).is_ok());
    let only_agent = |name: &str| name == "agent";
    let err = ensure_agent_binary_available_with(AgentKind::Cursor, "cursor", only_agent)
        .unwrap_err();
    assert_eq!(err.to_string(), "Agent 'cursor' not found: binary 'cursor-agent' missing from PATH");
}

#[test]
fn ensure_agent_binary_available_reports_missing_path_binary() {
    let err = ensure_agent_binary_available_with(AgentKind::Kilo, "kilo", |_| false)
        .unwrap_err();

    assert_eq!(
        err.to_string(),
        "Agent 'kilo' not found: binary 'kilo' missing from PATH"
    );
}

#[test]
fn ensure_resolved_binary_available_names_missing_custom_binary() {
    let err = ensure_resolved_binary_available_with("goose", "goose", |_| false).unwrap_err();

    assert_eq!(
        err.to_string(),
        "Agent 'goose' not found: binary 'goose' missing from PATH"
    );
}

#[test]
fn ensure_resolved_binary_available_rejects_missing_absolute_path() {
    let err = ensure_resolved_binary_available_with(
        "goose",
        "/definitely/missing/goose-bin",
        |_| true,
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("binary 'goose-bin' missing from PATH"),
        "unexpected error: {err}"
    );
}
