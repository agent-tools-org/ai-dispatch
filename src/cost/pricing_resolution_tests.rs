// Lookup-order tests: static catalog prices, no free-suffix guess, override, vendor-only feed.
// Deps: super::resolve_model_pricing, crate::cost::estimate_cost, AGENT_MODELS.

use super::*;
use crate::cost::{clear_feed_for_tests, estimate_cost, format_cost, format_cost_label};
use crate::model_catalog::AGENT_MODELS;
use crate::paths::AidHomeGuard;
use crate::types::AgentKind;
use tempfile::TempDir;

fn isolated() -> (TempDir, AidHomeGuard) {
    let temp = tempfile::tempdir().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    clear_feed_for_tests();
    (temp, guard)
}

#[test]
fn every_static_opencode_row_uses_its_own_catalog_price() {
    let _guard = isolated();
    let rows: Vec<_> = AGENT_MODELS
        .iter()
        .filter(|row| row.agent == AgentKind::OpenCode)
        .collect();
    assert!(
        !rows.is_empty(),
        "static catalog must contain opencode rows"
    );
    for row in rows {
        let pricing = resolve_model_pricing(row.model, AgentKind::OpenCode)
            .unwrap_or_else(|| panic!("{} must resolve from its static catalog row", row.model));
        assert_eq!(
            pricing.input_per_m, row.input_per_m,
            "{} input_per_m must match the static catalog row",
            row.model
        );
        assert_eq!(
            pricing.output_per_m, row.output_per_m,
            "{} output_per_m must match the static catalog row",
            row.model
        );
        let expected = row.input_per_m * 0.7 + row.output_per_m * 0.3;
        let cost = estimate_cost(1_000_000, Some(row.model), AgentKind::OpenCode)
            .unwrap_or_else(|| panic!("{} estimate_cost must not be unknown", row.model));
        assert!(
            (cost - expected).abs() < 1e-9,
            "{}: estimate_cost {cost} != catalog blended {expected}",
            row.model
        );
    }
}

#[test]
fn free_suffix_is_not_a_price() {
    let _guard = isolated();
    for model in ["opencode-go/hy3-free", "opencode/laguna-s-2.1-free", "opencode/laguna-free-v2"] {
        let cost = estimate_cost(100_000, Some(model), AgentKind::OpenCode);
        assert_eq!(cost, None, "{model}");
        assert_eq!(format_cost(cost), "unknown");
    }
}

fn write_served_codex(models: &[&str]) {
    crate::paths::ensure_dirs().unwrap();
    let now = chrono::Utc::now().timestamp();
    let cache = serde_json::json!({"codex": {"models": models, "updated_at_secs": now}});
    std::fs::write(crate::paths::aid_dir().join("served_models_cache.json"), cache.to_string()).unwrap();
}

#[test]
fn uncatalogued_model_not_in_the_feed_is_unknown_served_or_not() {
    let _guard = isolated();
    assert!(resolve_model_pricing("gpt-5.7-sol", AgentKind::Codex).is_none());
    write_served_codex(&["gpt-5.7-sol"]);
    assert!(resolve_model_pricing("gpt-5.7-sol", AgentKind::Codex).is_none());
    let cost = estimate_cost(1_000_000, Some("gpt-5.7-sol"), AgentKind::Codex);
    assert_eq!(cost, None);
    assert_eq!(format_cost(cost), "unknown");
}

#[test]
fn served_only_model_keeps_explicit_override_and_exact_feed_prices() {
    let _guard = isolated();
    write_served_codex(&["gpt-5.7-sol", "gpt-5.7-terra"]);
    let overrides = serde_json::json!({"models": [{
        "agent": "codex", "model": "gpt-5.7-sol", "input_per_m": 3.0, "output_per_m": 18.0,
        "tier": "premium", "description": "operator price", "updated": "2026-09-24"
    }]});
    std::fs::write(crate::paths::pricing_path(), overrides.to_string()).unwrap();
    let feed = serde_json::json!({
        "built_at": chrono::Utc::now().to_rfc3339(), "age_seconds": 1, "stale": false, "count": 1,
        "models": [{"id": "gpt-5.7-terra", "input_per_mtok": 4.0, "output_per_mtok": 20.0,
            "cached_input_per_mtok": null, "context_length": null, "source": null}]
    });
    std::fs::write(crate::paths::aid_dir().join("prices.json"), feed.to_string()).unwrap();
    clear_feed_for_tests();
    let sol = resolve_model_pricing("gpt-5.7-sol", AgentKind::Codex).expect("override");
    assert_eq!((sol.input_per_m, sol.output_per_m), (3.0, 18.0));
    let terra = resolve_model_pricing("gpt-5.7-terra", AgentKind::Codex).expect("exact feed");
    assert_eq!((terra.input_per_m, terra.output_per_m), (4.0, 20.0));
    clear_feed_for_tests();
}

#[test]
fn zero_figure_on_a_paid_tier_row_is_unknown_not_free() {
    let _guard = isolated();
    for (model, agent) in [
        ("claude-opus-5", AgentKind::Droid),
        ("inkling", AgentKind::Droid),
        ("auto", AgentKind::Oz),
        ("grok-4.6", AgentKind::Grok),
    ] {
        let cost = estimate_cost(1_000_000, Some(model), agent);
        assert_eq!(cost, None, "{agent:?}/{model}");
        assert_eq!(format_cost(cost), "unknown", "{agent:?}/{model}");
    }
}

#[test]
fn free_tier_and_subscription_rows_stay_zero() {
    let _guard = isolated();
    for (model, agent) in [
        ("gemini-3.1-pro-high", AgentKind::Antigravity),
        ("coder-model", AgentKind::Qwen),
        ("opencode/deepseek-v4-flash-free", AgentKind::OpenCode),
        ("kilo/kilo-auto/free", AgentKind::Kilo),
        ("mimo/mimo-auto", AgentKind::MiMoCode),
        ("gpt-5.4-high", AgentKind::Cursor),
    ] {
        assert_eq!(estimate_cost(1_000_000, Some(model), agent), Some(0.0), "{agent:?}/{model}");
    }
    let label = |model, agent| format_cost_label(estimate_cost(1_000_000, Some(model), agent), agent);
    assert_eq!(label("kilo/kilo-auto/free", AgentKind::Kilo), "included");
    assert_eq!(label("mimo/mimo-auto", AgentKind::MiMoCode), "included");
    assert_eq!(label("auto", AgentKind::Cursor), "subscription");
    assert_eq!(label("any-copilot-model", AgentKind::Copilot), "subscription");
}

fn write_overrides(rows: &[(&str, &str, f64, f64)]) {
    crate::paths::ensure_dirs().unwrap();
    let models: Vec<_> = rows.iter().map(|(agent, model, input, output)| serde_json::json!({
        "agent": agent, "model": model, "input_per_m": input, "output_per_m": output,
        "tier": "premium", "description": "operator price", "updated": "2026-09-30"
    })).collect();
    let file = serde_json::json!({ "models": models });
    std::fs::write(crate::paths::pricing_path(), file.to_string()).unwrap();
    clear_feed_for_tests();
}

#[test]
fn explicit_override_wins_over_catalog_subscription_and_unknown() {
    let _guard = isolated();
    write_overrides(&[
        ("codex", "GPT-5.6-SOL", 1.0, 2.0),
        ("cursor", "composer-2.5", 5.0, 5.0),
        ("droid", "claude-opus-5", 10.0, 10.0),
    ]);
    let price = |model, agent| {
        let p = resolve_model_pricing(model, agent).expect("override");
        (p.input_per_m, p.output_per_m)
    };
    assert_eq!(price("gpt-5.6-sol", AgentKind::Codex), (1.0, 2.0), "beats catalog row");
    assert_eq!(price("composer-2.5", AgentKind::Cursor), (5.0, 5.0), "beats subscription");
    assert_eq!(price("claude-opus-5", AgentKind::Droid), (10.0, 10.0), "prices a 0/0 row");
    let cost = estimate_cost(1_000_000, Some("composer-2.5"), AgentKind::Cursor);
    assert_eq!(format_cost_label(cost, AgentKind::Cursor), "$5.00");
    assert!(crate::cost::has_known_price(Some("claude-opus-5"), AgentKind::Droid));
    // Other models of the same agents keep their normal resolution.
    assert_eq!(estimate_cost(1_000_000, Some("auto"), AgentKind::Cursor), Some(0.0));
    assert_eq!(estimate_cost(1_000_000, Some("gpt-5.5"), AgentKind::Droid), None);
    clear_feed_for_tests();
}

#[test]
fn vendor_feed_rate_prices_only_the_vendor_cli_not_a_reseller() {
    let _guard = isolated();
    crate::paths::ensure_dirs().unwrap();
    let feed = serde_json::json!({
        "built_at": chrono::Utc::now().to_rfc3339(), "age_seconds": 1, "stale": false, "count": 1,
        "models": [{"id": "claude-sonnet-4-6", "input_per_mtok": 3.0, "output_per_mtok": 15.0,
            "cached_input_per_mtok": null, "context_length": null, "source": null}]
    });
    std::fs::write(crate::paths::aid_dir().join("prices.json"), feed.to_string()).unwrap();
    clear_feed_for_tests();
    let own = resolve_model_pricing("claude-sonnet-4-6", AgentKind::Claude).expect("vendor feed");
    assert_eq!((own.input_per_m, own.output_per_m), (3.0, 15.0));
    for agent in [AgentKind::Droid, AgentKind::Oz, AgentKind::OpenCode] {
        assert_eq!(estimate_cost(1_000_000, Some("claude-sonnet-4-6"), agent), None, "{agent:?}");
        assert!(!crate::cost::has_known_price(Some("claude-sonnet-4-6"), agent), "{agent:?}");
    }
    clear_feed_for_tests();
}
