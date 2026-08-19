// Model-group quota tests. Model ids and the quota message are verbatim from
// `agy models` and a live agy run on 2026-08-05.

use super::*;

#[test]
fn agy_models_map_to_their_families() {
    let agy = AgentKind::Antigravity;
    assert_eq!(model_group(agy, Some("gemini-3.6-flash-low")), Some("gemini"));
    assert_eq!(model_group(agy, Some("claude-sonnet-4-6")), Some("claude"));
    assert_eq!(model_group(agy, Some("gpt-oss-120b-medium")), Some("gpt-oss"));
}

/// Cursor meters one premium pool. Enumerating the premium models left every
/// model outside the day's evidence reading as unmetered, so after the pool was
/// spent aid kept dispatching to it. `auto` is the only exception.
#[test]
fn every_cursor_model_but_auto_draws_on_the_premium_pool() {
    let cursor = AgentKind::Cursor;
    for model in [
        "composer-2.5",
        "gpt-5.4-high",
        // None of these appeared in the evidence that motivated the fix; all of
        // them still spend the same pool.
        "claude-opus-5-thinking",
        "claude-opus-5-thinking-high",
        "gemini-3.1-pro",
        "o3",
        "composer-2",
    ] {
        assert_eq!(
            model_group(cursor, Some(model)),
            Some("premium"),
            "{model} must be held with the premium pool"
        );
    }
}

/// The other half of the same fact: after a premium refusal `auto` keeps
/// serving, so it must not be marked with the pool that ran out.
#[test]
fn cursor_auto_is_not_held_with_the_premium_pool() {
    assert_eq!(model_group(AgentKind::Cursor, Some("auto")), Some("auto"));
    assert_eq!(model_group(AgentKind::Cursor, Some("Auto")), Some("auto"));
}

/// The refusal captured on t-dfc23e80, t-b38df7a8 and t-d6fef491. Every path
/// that writes a marker without a model in hand reads the tier from this string;
/// if it stops being recognised, those paths silently go back to marking the
/// whole agent and `auto` goes out with the premium pool.
#[test]
fn the_cursor_premium_refusal_names_its_own_tier() {
    assert_eq!(
        group_from_refusal(
            AgentKind::Cursor,
            "ActionRequiredError: Increase limits for faster responses You're out of usage. \
             Switch to Auto, or ask your admin to increase your limit to continue."
        ),
        Some("premium")
    );
}

/// A workspace cap is not a tier cap, and no other agent has tiers to name.
/// Both must fall through to agent-level marking unchanged.
#[test]
fn a_refusal_that_names_no_tier_stays_agent_wide() {
    assert_eq!(
        group_from_refusal(AgentKind::Cursor, "Quota exceeded for this workspace"),
        None
    );
    assert_eq!(group_from_refusal(AgentKind::Cursor, "HTTP 429 Too Many Requests"), None);
    assert_eq!(
        group_from_refusal(AgentKind::Codex, "You're out of usage. Switch to Auto."),
        None
    );
}

#[test]
fn a_spent_cursor_premium_pool_falls_back_to_auto() {
    let got = healthy_model_for(AgentKind::Cursor, Some("composer-2.5"), |group| {
        group == "premium"
    });
    assert_eq!(got, Some("auto"));
}

/// Cursor's tier split must not be bought by rewriting its metering shape:
/// a subscription is what it is, and the pricing layer reads that field.
#[test]
fn cursor_stays_a_subscription_while_still_being_grouped() {
    assert_eq!(
        crate::types::provider_for_cli(AgentKind::Cursor).1,
        crate::types::MeteringShape::Subscription
    );
    assert!(has_grouped_quota(AgentKind::Cursor));
}

#[test]
fn agents_metered_per_account_have_no_groups() {
    assert_eq!(model_group(AgentKind::Qwen, Some("qwen3.8-max")), None);
    assert_eq!(model_group(AgentKind::Codex, Some("gpt-5.5")), None);
    assert!(groups_for_agent(AgentKind::Codex).is_empty());
}

#[test]
fn exhausted_gemini_group_switches_to_claude() {
    let got = healthy_model_for(
        AgentKind::Antigravity,
        Some("gemini-3.6-flash-low"),
        |group| group == "gemini",
    );
    assert_eq!(got, Some("claude-opus-4-6-thinking"));
}

#[test]
fn a_healthy_current_group_is_left_alone() {
    // No churn: an unrelated group's outage must not move work off a model that
    // is still serving.
    let got = healthy_model_for(
        AgentKind::Antigravity,
        Some("claude-sonnet-4-6"),
        |group| group == "gemini",
    );
    assert_eq!(got, None);
}

#[test]
fn every_group_exhausted_yields_nothing_to_switch_to() {
    let got = healthy_model_for(AgentKind::Antigravity, Some("gemini-3.1-pro-high"), |_| true);
    assert_eq!(got, None);
}

#[test]
fn no_current_model_still_finds_a_healthy_group() {
    let got = healthy_model_for(AgentKind::Antigravity, None, |group| group == "gemini");
    assert_eq!(got, Some("claude-opus-4-6-thinking"));
}

/// Captured 2026-08-19 12:06-12:10 with the standard pool exhausted.
const DROID_WEEKLY_402: &str = r#"{"type":"error","source":"agent_loop","message":"402 {\"detail\":\"You've reached your weekly standard usage limit (resets in 2 days).\nSwitch to Droid Core or enable Extra Usage to continue.\",\"status\":402,\"title\":\"Payment Required\",\"displayToUser\":true}"}"#;

#[test]
fn droid_core_follows_factory_billing_pool() {
    let droid = AgentKind::Droid;
    for model in [
        "glm-5.2",
        "glm-5.2-fast",
        "kimi-k3",
        "kimi-k2.7-code",
        "kimi-k2.6",
        "deepseek-v4-flash-0731",
        "deepseek-v4-pro",
        "minimax-m3",
        "minimax-m2.7",
        "inkling",
        "nemotron-3-ultra",
        "GLM-5.2",
        // deprecated / availableInCLI:!1, still billingPool:"core"
        "glm-4.6",
        "shield-risk",
    ] {
        assert_eq!(
            model_group(droid, Some(model)),
            Some("core"),
            "{model} is billingPool:core"
        );
    }
}

/// inkling and nemotron-3-ultra were not in the 2026-08-19 probe. Factory
/// still labels both billingPool:"core" and CLI-selectable. Narrowing the
/// allowlist back to the four probed ids must fail here.
#[test]
fn an_unprobed_cli_core_model_is_still_core() {
    assert_eq!(model_group(AgentKind::Droid, Some("inkling")), Some("core"));
    assert_eq!(
        model_group(AgentKind::Droid, Some("nemotron-3-ultra")),
        Some("core")
    );
}

#[test]
fn every_other_droid_name_is_standard_including_unknown_and_none() {
    let droid = AgentKind::Droid;
    for model in [
        "claude-opus-5",
        "claude-haiku-4-5-20251001",
        "gpt-5.6-luna",
        "grok-4.6",
        "auto",
        "not-a-real-model",
    ] {
        assert_eq!(
            model_group(droid, Some(model)),
            Some("standard"),
            "{model} must draw on the standard pool"
        );
    }
    assert_eq!(model_group(droid, None), Some("standard"));
}

#[test]
fn the_droid_standard_refusal_names_its_own_tier() {
    assert_eq!(
        group_from_refusal(AgentKind::Droid, DROID_WEEKLY_402),
        Some("standard")
    );
    assert_eq!(
        group_from_refusal(
            AgentKind::Droid,
            "You've reached your 5-hour standard usage limit (resets in 1h 48min)."
        ),
        Some("standard")
    );
}

#[test]
fn a_droid_refusal_that_names_no_tier_stays_agent_wide() {
    assert_eq!(
        group_from_refusal(AgentKind::Droid, "402 payment required: reload your tokens"),
        None
    );
}

#[test]
fn a_spent_droid_standard_pool_falls_back_to_core() {
    let got = healthy_model_for(AgentKind::Droid, Some("claude-opus-5"), |group| {
        group == "standard"
    });
    assert_eq!(got, Some("glm-5.2"));
}

#[test]
fn droid_stays_an_account_pool_while_still_being_grouped() {
    assert_eq!(
        crate::types::provider_for_cli(AgentKind::Droid).1,
        crate::types::MeteringShape::AccountPool
    );
    assert!(has_grouped_quota(AgentKind::Droid));
}
