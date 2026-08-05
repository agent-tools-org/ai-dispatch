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
