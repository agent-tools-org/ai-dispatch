// Tests for quota visibility on `aid agent list` / `aid agent quota`.
// Deps: agent_display helpers, rate_limit markers, AidHomeGuard.

use super::*;
use crate::paths::{self, AidHomeGuard};
use crate::rate_limit::{
    clear_all_rate_limits_for_agent, is_rate_limited, mark_group_rate_limited, mark_rate_limited,
};
use crate::types::AgentKind;

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
        .map(|k| quota_row(*k))
        .collect();
    assert!(rows.iter().all(|r| matches!(r, QuotaRow::Ok)));
}

#[test]
fn agent_level_hold_is_limited_not_partial() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_rate_limited(
        &AgentKind::Codex,
        "You've hit your usage limit. try again at Aug 11th, 2099 2:23 PM.",
    );

    match quota_row(AgentKind::Codex) {
        QuotaRow::Limited { detail } => {
            assert!(detail.contains("resets Aug 11th, 2099 2:23 PM"), "{detail}");
            assert!(!detail.contains("~1h"), "must not invent a reset time");
        }
        other => panic!("expected Limited, got {other:?}"),
    }
    assert!(matches!(quota_row(AgentKind::Gemini), QuotaRow::Ok));
}

#[test]
fn cursor_premium_group_hold_is_partial_and_names_clear_limit() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_group_rate_limited(
        &AgentKind::Cursor,
        "premium",
        "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.",
    );

    assert!(
        !is_rate_limited(&AgentKind::Cursor),
        "group hold must not mark the agent"
    );
    match quota_row(AgentKind::Cursor) {
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

    clear_all_rate_limits_for_agent(&AgentKind::Cursor);
    assert!(matches!(quota_row(AgentKind::Cursor), QuotaRow::Ok));
}

#[test]
fn human_ended_agent_hold_does_not_invent_reset_time() {
    let temp = isolated();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_rate_limited(
        &AgentKind::OpenCode,
        "APIError: Insufficient balance. Manage your billing here",
    );

    match quota_row(AgentKind::OpenCode) {
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
