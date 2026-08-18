// Lookup-order tests: static catalog prices and the uniform free-suffix rule.
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
