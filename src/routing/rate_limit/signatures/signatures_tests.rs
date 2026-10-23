// Tests for per-CLI quota signatures. Messages below are captured CLI output,
// not invented envelopes that let an adapter fix miss production.

use super::*;
use super::recovery::parse_relative_recovery;
use chrono::Local;

#[test]
fn droid_weekly_limit_is_recognized_with_a_day_long_cooldown() {
    let msg = "402 You've reached your weekly standard usage limit (resets in 1 day).\nSwitch to Droid Core or enable Extra Usage to continue.";
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(msg, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("droid quota message must match");
    assert_eq!(agent, AgentKind::Droid);
    assert_eq!(recovery, QuotaRecovery::After(1440));
}

#[test]
fn codex_usage_limit_is_recognized() {
    let msg = "You have hit your usage limit. Upgrade to Pro or try again at Mar 19th, 2026 2:27 PM.";
    let (agent, _) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(msg, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("codex quota message must match");
    assert_eq!(agent, AgentKind::Codex);
}

#[test]
fn ordinary_output_does_not_match_any_signature() {
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .find_map(
                |agent| crate::routing::rate_limit::signatures::match_quota_signature_for_agent(
                    "compiling ai-dispatch v10.0.0",
                    *agent
                )
                .map(|recovery| (*agent, recovery))
            )
            .is_none()
    );
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .find_map(
                |agent| crate::routing::rate_limit::signatures::match_quota_signature_for_agent(
                    "network timeout after 30s",
                    *agent
                )
                .map(|recovery| (*agent, recovery))
            )
            .is_none()
    );
}

#[test]
fn resets_in_one_day_parses_to_roughly_a_day_out() {
    let parsed = parse_relative_recovery("resets in 1 day").expect("must parse");
    let delta = parsed - Local::now().naive_local();
    assert!(delta.num_hours() >= 23 && delta.num_hours() <= 24, "got {delta:?}");
}

#[test]
fn resets_in_minutes_and_hours_parse() {
    let minutes = parse_relative_recovery("please wait, resets in 45 minutes").expect("minutes");
    assert!((minutes - Local::now().naive_local()).num_minutes() >= 44);

    let hours = parse_relative_recovery("try again in 3 hours").expect("hours");
    assert!((hours - Local::now().naive_local()).num_hours() >= 2);
}

#[test]
fn hyphenated_window_is_ignored_without_a_reset_phrase() {
    assert!(parse_relative_recovery("your token-plan 5-hour quota has been exhausted").is_none());
}

#[test]
fn messages_without_a_reset_hint_return_none() {
    assert!(parse_relative_recovery("quota exhausted").is_none());
}

#[test]
fn oz_quota_limit_reached_is_recognized() {
    // Verbatim from `oz agent run`, exit code 1. Note it matches none of the
    // other four providers' wordings — "quota exceeded", "usage limit",
    // "quota has been exhausted" and "individual quota reached" all miss it.
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(
                "Error: Quota limit reached.",
                *agent,
            )
            .map(|recovery| (*agent, recovery))
        })
        .expect("oz quota message must match");
    assert_eq!(agent, AgentKind::Oz);
    assert_eq!(recovery, QuotaRecovery::After(60));
}

#[test]
fn oz_invalid_credentials_is_recognized() {
    // Verbatim oz CLI stderr when logged out. Same channel as the quota line
    // above — do not treat agent-authored prose that quotes this as a hold.
    let msg = "Error: Your credentials are invalid. Please log in again with `oz login`.";
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(msg, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("oz credentials message must match");
    assert_eq!(agent, AgentKind::Oz);
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
    assert_eq!(
        match_quota_signature_for_agent(msg, AgentKind::Oz),
        Some(QuotaRecovery::NeedsHuman)
    );
}

#[test]
fn prose_that_merely_mentions_quota_is_not_a_quota_failure() {
    // Verbatim from docs/design/cli-adapter-audit.md, which an agent read during
    // a task. A bare "quota" needle matched it and locked the agent out for
    // twelve hours while the task had actually succeeded.
    let doc = "be exercised (no credentials, exhausted quota), say so explicitly \
               — an honest gap is worth more than an assumed pass.";
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .find_map(
                |agent| crate::routing::rate_limit::signatures::match_quota_signature_for_agent(doc, *agent)
                    .map(|recovery| (*agent, recovery))
            )
            .is_none(),
        "documentation prose must not match a provider quota signature"
    );
}

#[test]
fn droid_weekly_core_and_standard_needles_do_not_shadow_each_other() {
    let core = r#"402 {"detail":"You've reached your weekly Droid Core usage limit (resets in 2 days).\nReload Extra Usage credits or wait for your limits to reset.","status":402,"title":"Payment Required","displayToUser":true}"#;
    let standard = "402 You've reached your weekly standard usage limit (resets in 1 day).\nSwitch to Droid Core or enable Extra Usage to continue.";

    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(core, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("core quota message must match");
    assert_eq!(agent, AgentKind::Droid);
    assert_eq!(recovery, QuotaRecovery::After(2880));

    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(standard, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("standard quota message must match");
    assert_eq!(agent, AgentKind::Droid);
    assert_eq!(recovery, QuotaRecovery::After(1440));

    let core_l = core.to_lowercase();
    let standard_l = standard.to_lowercase();
    assert!(core_l.contains("weekly droid core usage limit"));
    assert!(!core_l.contains("weekly standard usage limit"));
    assert!(standard_l.contains("weekly standard usage limit"));
    assert!(!standard_l.contains("weekly droid core usage limit"));
}

#[test]
fn droid_reload_tokens_is_recognized() {
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(
                "402 payment required: reload your tokens",
                *agent,
            )
            .map(|recovery| (*agent, recovery))
        })
        .expect("droid reload-tokens message must match");
    assert_eq!(agent, AgentKind::Droid);
    // Reloading tokens is a purchase, not a window elapsing.
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
}

#[test]
fn cursor_workspace_quota_is_recognized() {
    let (agent, _) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(
                "quota exceeded for this workspace",
                *agent,
            )
            .map(|recovery| (*agent, recovery))
        })
        .expect("cursor workspace quota must match");
    assert_eq!(agent, AgentKind::Cursor);
}

#[test]
fn agent_scoped_match_ignores_other_providers() {
    assert!(match_quota_signature_for_agent(
        "You have hit your usage limit.",
        AgentKind::Codex
    )
        .is_some());
    assert!(match_quota_signature_for_agent(
        "You have hit your usage limit.",
        AgentKind::Cursor
    )
        .is_none());
}

#[test]
fn opencode_insufficient_balance_is_recognized() {
    // Verbatim from t-76181278's log: an HTTP 401 body, not a 429 or 402, so
    // generic phrase matching would miss it. aid reported opencode as OK and
    // kept dispatching to an account that could not pay.
    let body = r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here: https://opencode.ai/workspace/wrk_01/billing","statusCode":401}}}"#;
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter()
        .chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(body, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("opencode balance message must match");
    assert_eq!(agent, AgentKind::OpenCode);
    // A balance ends when the account is topped up, never on a clock.
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
}

#[test]
fn a_balance_failure_reads_as_a_rate_limit_error() {
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "APIError: Insufficient balance. Manage your billing here: https://opencode.ai/",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
}

#[test]
fn insufficient_balance_prefers_dispatched_overlay_agent() {
    let body = "APIError: Insufficient balance. Manage your billing here";
    let recovery = match_quota_signature_for_agent(body, AgentKind::MiMoCode)
        .expect("mimocode balance message must match");
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
    let recovery =
        match_quota_signature_for_agent(body, AgentKind::Kilo).expect("kilo balance message must match");
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
}

#[test]
fn copilot_premium_request_limit_is_recognized() {
    let msg = "You've reached your premium request limit for this billing cycle.";
    assert!(match_quota_signature_for_agent(msg, AgentKind::Copilot).is_some());
}

#[path = "signatures_tests_entry_tests.rs"]
mod production_entries;
