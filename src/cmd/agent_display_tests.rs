// Tests for quota visibility on `aid agent list` / `aid agent quota`.
// Deps: agent_display helpers, rate_limit markers, AidHomeGuard.

use super::*;
use crate::paths::{self, AidHomeGuard};
use crate::rate_limit::{
    clear_all_rate_limits_for_agent, is_rate_limited, mark_group_rate_limited, mark_rate_limited,
};
use crate::types::AgentKind;

fn custom_agent_toml(id: &str) -> String {
    format!(
        "[agent]\nid = \"{id}\"\ndisplay_name = \"{id} agent\"\ncommand = \"{id}\"\n"
    )
}

fn write_custom_agent(aid_home: &std::path::Path, id: &str) {
    let agents_dir = aid_home.join(".aid").join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(agents_dir.join(format!("{id}.toml")), custom_agent_toml(id)).unwrap();
}

fn isolated() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let _ = std::fs::create_dir_all(temp.path().join(".aid"));
    temp
}

#[test]
fn healthy_list_omits_status_column() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    let rows: Vec<_> = AgentKind::ALL_BUILTIN
        .iter()
        .map(|k| quota_row(*k, None))
        .collect();
    assert!(rows.iter().all(|r| matches!(r, QuotaRow::Ok)));
}

#[test]
fn agent_level_hold_is_limited_not_partial() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    let stated = crate::rate_limit::test_future_recovery_time();
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!("You've hit your usage limit. try again at {stated}."),
    );

    match quota_row(AgentKind::Codex, None) {
        QuotaRow::Limited { detail } => {
            assert!(detail.contains(&format!("resets {stated}")), "{detail}");
            assert!(!detail.contains("~1h"), "must not invent a reset time");
        }
        other => panic!("expected Limited, got {other:?}"),
    }
    assert!(matches!(quota_row(AgentKind::Gemini, None), QuotaRow::Ok));
}

#[test]
fn cursor_premium_group_hold_is_partial_and_names_clear_limit() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_group_rate_limited(&AgentKind::Cursor, None,
        "premium",
        "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.",
    );

    assert!(
        !is_rate_limited(&AgentKind::Cursor, None),
        "group hold must not mark the agent"
    );
    match quota_row(AgentKind::Cursor, None) {
        QuotaRow::Partial { detail } => {
            assert!(detail.contains("premium"), "{detail}");
            assert!(
                detail.contains("aid config clear-limit cursor"),
                "manual hold must name the clear command: {detail}"
            );
            assert!(!detail.contains("~1h"), "{detail}");
        }
        other => panic!("expected Partial, got {other:?}"),
    }

    clear_all_rate_limits_for_agent(&AgentKind::Cursor, None);
    assert!(matches!(quota_row(AgentKind::Cursor, None), QuotaRow::Ok));
}

#[test]
fn group_hold_detail_includes_provider_message() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_group_rate_limited(&AgentKind::Cursor, None,
        "premium",
        "ActionRequiredError: ask your admin to increase your limit to continue.",
    );

    match quota_row(AgentKind::Cursor, None) {
        QuotaRow::Partial { detail } => {
            assert!(detail.contains("premium"), "{detail}");
            assert!(
                detail.contains("ActionRequiredError"),
                "provider message must appear in group hold detail: {detail}"
            );
        }
        other => panic!("expected Partial, got {other:?}"),
    }
}

#[test]
fn human_ended_agent_hold_does_not_invent_reset_time() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_rate_limited(&AgentKind::OpenCode, None,
        "APIError: Insufficient balance. Manage your billing here",
    );

    match quota_row(AgentKind::OpenCode, None) {
        QuotaRow::Limited { detail } => {
            assert!(
                detail.contains("aid config clear-limit opencode"),
                "{detail}"
            );
            assert!(!detail.contains("resets"), "{detail}");
            assert!(!detail.contains("~1h"), "{detail}");
        }
        other => panic!("expected Limited, got {other:?}"),
    }
}

#[test]
fn custom_agent_with_hold_shows_limited() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    let stated = crate::rate_limit::test_future_recovery_time();
    mark_rate_limited(
        &AgentKind::Custom,
        Some("auditor"),
        &format!("try again at {stated}."),
    );

    match quota_row(AgentKind::Custom, Some("auditor")) {
        QuotaRow::Limited { detail } => {
            assert!(detail.contains(&format!("resets {stated}")), "{detail}");
        }
        other => panic!("expected Limited, got {other:?}"),
    }
    // An unrelated custom agent must not inherit the hold.
    assert!(
        matches!(quota_row(AgentKind::Custom, Some("other-agent")), QuotaRow::Ok),
        "unrelated custom agent must not inherit the hold"
    );
    // Built-in agents must be unaffected.
    assert!(
        matches!(quota_row(AgentKind::Codex, None), QuotaRow::Ok),
        "built-in codex must not inherit custom agent hold"
    );
}

/// A manually-held custom agent's clear-limit hint must name the agent's own
/// id (e.g. "clear-limit auditor"), never the kind constant "clear-limit custom".
/// Before this was enforced, `AgentKind::Custom.as_str()` ("custom") leaked
/// into the hint, making the displayed command wrong.
#[test]
fn custom_agent_human_hold_names_agent_id_in_clear_limit_hint() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    // A message with no reset date causes needs_human = true, so format_hold_end
    // emits the "held until cleared with `aid config clear-limit <slug>`" line.
    mark_rate_limited(
        &AgentKind::Custom,
        Some("auditor"),
        "APIError: Insufficient balance. Manage your billing here",
    );

    match quota_row(AgentKind::Custom, Some("auditor")) {
        QuotaRow::Limited { detail } => {
            assert!(
                detail.contains("clear-limit auditor"),
                "hint must name the agent id 'auditor', got: {detail}"
            );
            assert!(
                !detail.contains("clear-limit custom"),
                "hint must NOT say 'clear-limit custom', got: {detail}"
            );
        }
        other => panic!("expected Limited, got {other:?}"),
    }
}

/// The `show_quota` and `list_agents` printer loops must handle a held custom
/// agent without panicking and must complete successfully. Tests here exercise
/// the loops themselves, not just the `quota_row` helper they call.
#[test]
fn show_quota_and_list_agents_cover_held_custom_agent_loop() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();
    // Register a custom agent so the loops have something to iterate over.
    write_custom_agent(temp.path(), "held-agent");
    mark_rate_limited(
        &AgentKind::Custom,
        Some("held-agent"),
        "APIError: Insufficient balance.",
    );

    let quota_result = show_quota();
    assert!(quota_result.is_ok(), "show_quota failed: {:?}", quota_result);

    let list_result = list_agents();
    assert!(list_result.is_ok(), "list_agents failed: {:?}", list_result);
}
