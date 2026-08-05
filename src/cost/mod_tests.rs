// Unit tests for cost estimation and formatting.
// Deps: super (cost::*)

use super::*;


#[test]
fn kilo_and_free_models_zero_cost() {
    assert_eq!(
        estimate_cost(
            100_000,
            Some("opencode/mimo-v2-flash-free"),
            AgentKind::OpenCode
        ),
        Some(0.0)
    );
    assert_eq!(estimate_cost(100_000, None, AgentKind::Kilo), Some(0.0));
    assert_eq!(estimate_cost(100_000, None, AgentKind::MiMoCode), Some(0.0));
    assert_eq!(
        estimate_cost(100_000, Some("kilo/kilo/auto-free"), AgentKind::Kilo),
        Some(0.0)
    );
    assert_eq!(
        estimate_cost(100_000, Some("mimo/mimo-auto"), AgentKind::MiMoCode),
        Some(0.0)
    );
}

#[test]
fn gpt41_cost_estimate() {
    let cost = estimate_cost(1_000_000, Some("gpt-4.1"), AgentKind::Codex).unwrap();
    assert!((cost - 3.8).abs() < 0.01);
}

#[test]
fn codex_fallback_uses_standard_tier_or_first_catalog_model() {
    let cost = estimate_cost(1_000_000, None, AgentKind::Codex).unwrap();
    // Mirrors codex_fallback_pricing: prefer a "standard" tier model, else the first.
    let models = model_catalog::models_for_agent(&AgentKind::Codex);
    let fallback = models
        .iter()
        .find(|m| m.tier == "standard")
        .or_else(|| models.first())
        .unwrap();
    let blended = fallback.input_per_m * 0.7 + fallback.output_per_m * 0.3;
    assert!((cost - blended).abs() < 0.01);
}

#[test]
fn composer2_cost_estimate() {
    let cost = estimate_cost(1_000_000, Some("composer-2"), AgentKind::Cursor).unwrap();
    assert!((cost - 1.10).abs() < 0.01);
}

#[test]
fn unknown_model_returns_none() {
    let cost = estimate_cost(1000, Some("unknown-model"), AgentKind::OpenCode);
    assert!(cost.is_none());
}

#[test]
fn format_cost_variants() {
    assert_eq!(format_cost(Some(0.0)), "free");
    assert_eq!(format_cost(Some(0.0038)), "$0.0038");
    assert_eq!(format_cost(Some(1.23)), "$1.23");
    assert_eq!(format_cost(None), "unknown");
}

#[test]
fn gpt56_matches_flagship_premium_rates() {
    let sol = model_pricing("gpt-5.6-sol", AgentKind::Codex).unwrap();
    assert_eq!(sol.input_per_m, 2.5);
    assert_eq!(sol.output_per_m, 15.0);
    let luna = model_pricing("gpt-5.6-luna", AgentKind::Codex).unwrap();
    assert_eq!(luna.input_per_m, 0.4);
    assert_eq!(luna.output_per_m, 1.6);
}

#[test]
fn format_cost_label_special_cases() {
    assert_eq!(format_cost_label(Some(1.0), AgentKind::Cursor), "$1.00");
    assert_eq!(format_cost_label(None, AgentKind::Cursor), "subscription");
    assert_eq!(format_cost_label(None, AgentKind::Copilot), "subscription");
    assert_eq!(format_cost_label(Some(0.0), AgentKind::Kilo), "included");
    assert_eq!(format_cost_label(Some(0.0), AgentKind::MiMoCode), "included");
}

#[test]
fn format_cost_label_codebuff() {
    assert_eq!(format_cost_label(Some(1.5), AgentKind::Codebuff), "$1.50");
}

#[test]
fn gemini_estimate_fallback_without_explicit_model_matches_gemini_three_flash_blend() {
    let blended =
        estimate_cost(1_000_000, None, AgentKind::Gemini).expect("gemini default pricing present");
    let expected = model_pricing("gemini-3-flash-preview", AgentKind::Gemini).unwrap();
    let blended_per_m = expected.input_per_m * 0.7 + expected.output_per_m * 0.3;
    assert!((blended - blended_per_m).abs() < 0.001);
}

#[test]
fn gemini_3_preview_model_pricing() {
    let p = model_pricing("gemini-3.1-pro-preview", AgentKind::Gemini).unwrap();
    assert_eq!(p.input_per_m, 1.25);
    assert_eq!(p.output_per_m, 10.0);
    let p = model_pricing("gemini-3-flash-preview", AgentKind::Gemini).unwrap();
    assert_eq!(p.input_per_m, 0.30);
    assert_eq!(p.output_per_m, 2.50);
    let p = model_pricing("gemini-3-flash-lite-preview", AgentKind::Gemini).unwrap();
    assert_eq!(p.input_per_m, 0.10);
    assert_eq!(p.output_per_m, 0.40);
}

#[test]
fn new_model_pricing_entries() {
    let pricing = model_pricing("claude-sonnet-4", AgentKind::Custom).unwrap();
    assert_eq!(pricing.input_per_m, 3.0);
    assert_eq!(pricing.output_per_m, 15.0);
    let pricing = model_pricing("gpt-5", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 1.25);
    assert_eq!(pricing.output_per_m, 10.0);
    let pricing = model_pricing("gpt-4.1", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 2.0);
    assert_eq!(pricing.output_per_m, 8.0);
    let pricing = model_pricing("gpt-5.4", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 2.5);
    assert_eq!(pricing.output_per_m, 15.0);
    let pricing = model_pricing("gpt-5.4-mini", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 0.4);
    assert_eq!(pricing.output_per_m, 1.6);
    let pricing = model_pricing("gpt-5.5", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 2.5);
    assert_eq!(pricing.output_per_m, 15.0);
    let pricing = model_pricing("gpt-5.5-mini", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 0.4);
    assert_eq!(pricing.output_per_m, 1.6);
    let pricing = model_pricing("gpt-5-mini", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 0.25);
    assert_eq!(pricing.output_per_m, 2.0);
    let pricing = model_pricing("o3-mini", AgentKind::Custom).unwrap();
    assert_eq!(pricing.input_per_m, 1.10);
    assert_eq!(pricing.output_per_m, 4.40);
}
