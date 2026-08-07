use super::{
    built_in_agent_binary_exists, ensure_agent_binary_available_with,
    ensure_resolved_binary_available_with,
};
use crate::types::AgentKind;

#[test]
fn built_in_agent_binary_exists_rejects_missing_kilo_binary() {
    assert!(!built_in_agent_binary_exists(AgentKind::Kilo, |_| false));
}

#[test]
fn built_in_agent_binary_exists_rejects_missing_mimocode_binary() {
    assert!(!built_in_agent_binary_exists(AgentKind::MiMoCode, |_| false));
}

#[test]
fn built_in_agent_binary_exists_accepts_cursor_alias_binary() {
    assert!(built_in_agent_binary_exists(AgentKind::Cursor, |name| {
        name == "cursor-agent"
    }));
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
