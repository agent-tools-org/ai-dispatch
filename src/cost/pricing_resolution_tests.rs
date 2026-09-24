// Lookup-order tests: static catalog prices, the free-suffix rule, and served-only models.
// Deps: super::resolve_model_pricing, crate::cost::estimate_cost, AGENT_MODELS.

use super::*;
use crate::cost::{clear_feed_for_tests, estimate_cost, format_cost};
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
fn free_suffix_outside_old_allowlist_prices_at_zero() {
    let _guard = isolated();
    assert_eq!(
        estimate_cost(100_000, Some("opencode-go/hy3-free"), AgentKind::OpenCode),
        Some(0.0)
    );
    assert_eq!(
        estimate_cost(
            100_000,
            Some("opencode/laguna-s-2.1-free"),
            AgentKind::OpenCode
        ),
        Some(0.0)
    );
}

#[test]
fn free_in_the_middle_of_a_name_stays_unknown() {
    let _guard = isolated();
    let cost = estimate_cost(
        100_000,
        Some("opencode/laguna-free-v2"),
        AgentKind::OpenCode,
    );
    assert_eq!(cost, None);
    assert_eq!(format_cost(cost), "unknown");
}

fn write_served_codex(models: &[&str]) {
    crate::paths::ensure_dirs().unwrap();
    let now = chrono::Utc::now().timestamp();
    let cache = serde_json::json!({"codex": {"models": models, "updated_at_secs": now}});
    std::fs::write(crate::paths::aid_dir().join("served_models_cache.json"), cache.to_string()).unwrap();
}

#[test]
fn served_only_model_gets_no_similar_name_price() {
    let _guard = isolated();
    // Unserved, the builtin substring matcher would price it like gpt-5.
    assert!(resolve_model_pricing("gpt-5.7-sol", AgentKind::Codex).is_some());
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
