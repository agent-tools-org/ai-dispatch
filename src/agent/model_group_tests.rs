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
