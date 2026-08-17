// Tests for per-CLI quota signatures. Every message below is verbatim captured
// CLI output, not an invented shape — inventing the envelope is what let the
// previous adapter fix pass its own tests while doing nothing in production.

use super::*;

#[test]
fn qwen_token_plan_exhaustion_is_recognized() {
    let msg = "Quota exhausted: Your token-plan 5-hour quota has been exhausted. The quota will reset later.";
    let (agent, recovery) = match_quota_signature(msg).expect("qwen quota message must match");
    assert_eq!(agent, AgentKind::Qwen);
    assert_eq!(recovery, QuotaRecovery::After(300));
}

#[test]
fn droid_weekly_limit_is_recognized_with_a_day_long_cooldown() {
    let msg = "402 You've reached your weekly standard usage limit (resets in 1 day).\nSwitch to Droid Core or enable Extra Usage to continue.";
    let (agent, recovery) = match_quota_signature(msg).expect("droid quota message must match");
    assert_eq!(agent, AgentKind::Droid);
    assert_eq!(recovery, QuotaRecovery::After(1440));
}

#[test]
fn codex_usage_limit_is_recognized() {
    let msg = "You have hit your usage limit. Upgrade to Pro or try again at Mar 19th, 2026 2:27 PM.";
    let (agent, _) = match_quota_signature(msg).expect("codex quota message must match");
    assert_eq!(agent, AgentKind::Codex);
}

#[test]
fn ordinary_output_does_not_match_any_signature() {
    assert!(match_quota_signature("compiling ai-dispatch v10.0.0").is_none());
    assert!(match_quota_signature("network timeout after 30s").is_none());
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
fn hyphenated_window_is_used_when_no_reset_time_is_given() {
    let parsed = parse_relative_recovery("your token-plan 5-hour quota has been exhausted").expect("must parse");
    let delta = parsed - Local::now().naive_local();
    assert!(delta.num_hours() >= 4 && delta.num_hours() <= 5, "got {delta:?}");
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
    let (agent, recovery) = match_quota_signature("Error: Quota limit reached.")
        .expect("oz quota message must match");
    assert_eq!(agent, AgentKind::Oz);
    assert_eq!(recovery, QuotaRecovery::After(60));
}

#[test]
fn prose_that_merely_mentions_quota_is_not_a_quota_failure() {
    // Verbatim from docs/design/cli-adapter-audit.md, which an agent read during
    // a task. A bare "quota" needle matched it and locked the agent out for
    // twelve hours while the task had actually succeeded.
    let doc = "be exercised (no credentials, exhausted quota), say so explicitly \
               — an honest gap is worth more than an assumed pass.";
    assert!(
        match_quota_signature(doc).is_none(),
        "documentation prose must not match a provider quota signature"
    );
}

#[test]
fn droid_reload_tokens_is_recognized() {
    let (agent, recovery) = match_quota_signature("402 payment required: reload your tokens")
        .expect("droid reload-tokens message must match");
    assert_eq!(agent, AgentKind::Droid);
    // Reloading tokens is a purchase, not a window elapsing.
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
}

#[test]
fn gemini_ineligible_tier_is_recognized() {
    let msg = "IneligibleTierError: This client is no longer supported for Gemini Code Assist for individuals; migrate to Antigravity";
    let (agent, recovery) = match_quota_signature(msg).expect("gemini tier message must match");
    assert_eq!(agent, AgentKind::Gemini);
    // The tier is retired, not throttled: no wait restores it.
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
}

#[test]
fn cursor_workspace_quota_is_recognized() {
    let (agent, _) = match_quota_signature("quota exceeded for this workspace")
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
    let (agent, recovery) = match_quota_signature(body).expect("opencode balance message must match");
    assert_eq!(agent, AgentKind::OpenCode);
    // A balance ends when the account is topped up, never on a clock.
    assert_eq!(recovery, QuotaRecovery::NeedsHuman);
}

#[test]
fn a_balance_failure_reads_as_a_rate_limit_error() {
    assert!(crate::rate_limit::is_rate_limit_error(
        "APIError: Insufficient balance. Manage your billing here: https://opencode.ai/"
    ));
}

#[test]
fn insufficient_balance_prefers_dispatched_overlay_agent() {
    let body = "APIError: Insufficient balance. Manage your billing here";
    let (agent, _) = match_quota_signature_with_agent(body, Some(AgentKind::MiMoCode))
        .expect("mimocode balance message must match");
    assert_eq!(agent, AgentKind::MiMoCode);
    let (agent, _) = match_quota_signature_with_agent(body, Some(AgentKind::Kilo))
        .expect("kilo balance message must match");
    assert_eq!(agent, AgentKind::Kilo);
}

#[test]
fn copilot_premium_request_limit_is_recognized() {
    let msg = "You've reached your premium request limit for this billing cycle.";
    assert!(match_quota_signature_for_agent(msg, AgentKind::Copilot).is_some());
}

#[test]
fn grok_usage_balance_exhausted_is_windowed() {
    let msg = "API error (status 402 Payment Required): Grok Build usage balance exhausted";
    let (agent, recovery) = match_quota_signature(msg).expect("grok 402 must match");
    assert_eq!(agent, AgentKind::Grok);
    assert_eq!(recovery, QuotaRecovery::Windowed);
}

#[test]
fn cursor_out_of_usage_is_windowed() {
    let msg = "ActionRequiredError: Increase limits for faster responses You're out of usage. \
               Switch to Auto, or ask your admin to increase your limit to continue.";
    let (agent, recovery) = match_quota_signature(msg).expect("cursor premium must match");
    assert_eq!(agent, AgentKind::Cursor);
    assert_eq!(recovery, QuotaRecovery::Windowed);
}
