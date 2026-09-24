// Unit tests for cost estimation and formatting: exact-match prices, unknown as None.
// Deps: super (cost::*)

use super::*;
use tempfile::TempDir;
use crate::paths::AidHomeGuard;

fn isolated() -> (TempDir, AidHomeGuard) {
    let temp = tempfile::tempdir().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    clear_feed_for_tests();
    (temp, guard)
}

#[test]
fn subscription_agents_stay_included_not_unknown() {
    let _guard = isolated();
    // Cursor/Copilot are flat-rate subscriptions: marginal cost is genuinely
    // ~0, and the model name must not flip that to "unknown".
    assert_eq!(
        estimate_cost(100_000, Some("composer-2"), AgentKind::Cursor),
        Some(0.0)
    );
    assert_eq!(
        estimate_cost(100_000, Some("some-copilot-model"), AgentKind::Copilot),
        Some(0.0)
    );
}

#[test]
fn unpinned_codex_default_without_observed_model_is_unknown() {
    let _guard = isolated();
    assert_eq!(estimate_cost(1_000_000, None, AgentKind::Codex), None);
}

#[test]
fn composer2_cursor_subscription_is_included() {
    let _guard = isolated();
    // Cursor is a flat-rate subscription: a named model costs ~0 marginal,
    // not the old blanket builtin 0.50/2.50. The "included" state is what the
    // three-state split must preserve for subscription agents.
    let cost = estimate_cost(1_000_000, Some("composer-2"), AgentKind::Cursor).unwrap();
    assert_eq!(cost, 0.0);
}

#[test]
fn unknown_model_returns_none() {
    let _guard = isolated();
    let cost = estimate_cost(1000, Some("unknown-model"), AgentKind::OpenCode);
    assert!(cost.is_none());
}

#[test]
fn discovered_agy_model_does_not_inherit_similar_model_pricing() {
    let _guard = isolated();
    clear_feed_for_tests();
    let cost = estimate_cost(
        1_000_000,
        Some("gemini-3.7-flash-high"),
        AgentKind::Antigravity,
    );
    assert_eq!(cost, None);
    assert_eq!(format_cost(cost), "unknown");
}

#[test]
fn discovered_opencode_model_does_not_inherit_similar_model_pricing() {
    let _guard = isolated();
    clear_feed_for_tests();
    let cost = estimate_cost(
        1_000_000,
        Some("opencode-go/glm-5.2"),
        AgentKind::OpenCode,
    );
    assert_eq!(cost, None);
    assert_eq!(format_cost(cost), "unknown");
}

#[test]
fn commandcode_unknown_model_stays_unknown() {
    let _guard = isolated();
    assert_eq!(
        estimate_cost(100_000, Some("nobody/has-this-model"), AgentKind::CommandCode),
        None
    );
}

#[test]
fn format_cost_variants() {
    assert_eq!(format_cost(Some(0.0)), "free");
    assert_eq!(format_cost(Some(0.0038)), "$0.0038");
    assert_eq!(format_cost(Some(1.23)), "$1.23");
    assert_eq!(format_cost(None), "unknown");
}

#[test]
fn format_cost_label_special_cases() {
    assert_eq!(format_cost_label(Some(1.0), AgentKind::Cursor), "$1.00");
    assert_eq!(format_cost_label(None, AgentKind::Cursor), "subscription");
    assert_eq!(format_cost_label(None, AgentKind::Copilot), "subscription");
    assert_eq!(format_cost_label(Some(0.0), AgentKind::Kilo), "included");
    assert_eq!(format_cost_label(Some(0.0), AgentKind::MiMoCode), "included");
}

/// A model the feed does not carry must resolve to `None` — never `Some(0.0)`
/// and never "free". That is the whole point of the three-state split.
#[test]
fn unknown_model_yields_none_not_zero() {
    let _guard = isolated();
    // `unknown-model` is absent from the catalog, overrides, and the feed.
    let cost = estimate_cost(1000, Some("unknown-model"), AgentKind::OpenCode);
    assert_eq!(cost, None);
    assert_eq!(format_cost(cost), "unknown");
    // A vendor-prefixed name nobody knows is equally unknown, not free.
    let cost = estimate_cost(1000, Some("nobody/has-this-model"), AgentKind::OpenCode);
    assert_eq!(cost, None);
    assert_eq!(format_cost(cost), "unknown");
}

#[test]
fn feed_reads_from_isolated_cache_file() {
    let (temp, _guard) = isolated();
    let feed = price_feed::Feed {
        built_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        age_seconds: Some(10),
        stale: Some(false),
        count: Some(1),
        models: vec![price_feed::FeedModel {
            id: "custom/feed-model-x".to_string(),
            aliases: vec![],
            input_per_mtok: 1.0,
            output_per_mtok: 2.0,
            cached_input_per_mtok: None,
            context_length: None,
            source: None,
        }],
    };
    let json = serde_json::to_vec(&feed).unwrap();
    std::fs::write(temp.path().join("prices.json"), json).unwrap();
    clear_feed_for_tests();

    let pricing = pricing_resolution::resolve_model_pricing("custom/feed-model-x", AgentKind::Codex).unwrap();
    assert_eq!(pricing.input_per_m, 1.0);
    assert_eq!(pricing.output_per_m, 2.0);
    clear_feed_for_tests();
}

#[test]
fn free_named_models_price_only_from_their_catalog_row() {
    let _guard = isolated();
    // Catalog rows are exact prices; a `-free` suffix alone prices nothing.
    assert_eq!(estimate_cost(100_000, Some("opencode/mimo-v2.5-free"), AgentKind::OpenCode), Some(0.0));
    assert_eq!(estimate_cost(100_000, Some("kilo/kilo-auto/free"), AgentKind::Kilo), Some(0.0));
    assert_eq!(estimate_cost(100_000, Some("mimo/mimo-auto"), AgentKind::MiMoCode), Some(0.0));
    assert_eq!(estimate_cost(100_000, Some("opencode/mimo-v2-flash-free"), AgentKind::OpenCode), None);
}

#[test]
fn unpinned_agents_without_an_observed_model_are_unknown() {
    let _guard = isolated();
    // No fixed fallback model: Gemini, Kilo and MiMoCode stay unknown.
    for agent in [AgentKind::Gemini, AgentKind::Kilo, AgentKind::MiMoCode, AgentKind::Claude] {
        assert_eq!(estimate_cost(1_000_000, None, agent), None, "{agent:?}");
    }
    assert_eq!(estimate_cost(1_000_000, None, AgentKind::Cursor), Some(0.0), "subscription");
}

#[test]
fn uncatalogued_model_absent_from_the_feed_is_unknown() {
    let _guard = isolated();
    // The old matcher priced any `gpt-5*` like gpt-5; exact matches only now.
    for model in ["gpt-5.7-sol", "gpt-4.1", "gpt-5", "claude-sonnet-4", "composer-2"] {
        assert_eq!(estimate_cost(1_000_000, Some(model), AgentKind::Codex), None, "{model}");
    }
    assert_eq!(estimate_cost(1_000_000, Some("gpt-5.6-sol"), AgentKind::CommandCode), None);
}

#[test]
fn catalog_rows_price_exactly() {
    let _guard = isolated();
    let sol = pricing_resolution::resolve_model_pricing("gpt-5.6-sol", AgentKind::Codex).expect("row");
    assert_eq!((sol.input_per_m, sol.output_per_m), (2.5, 15.0));
    let cost = estimate_cost(1_000_000, Some("gemini-3-flash-preview"), AgentKind::Gemini).expect("row");
    assert!((cost - (0.30 * 0.7 + 2.50 * 0.3)).abs() < 1e-9);
}

#[test]
fn exact_feed_match_prices_and_near_miss_does_not() {
    let _guard = isolated();
    use crate::cost::price_feed::{Feed, FeedModel};
    set_feed_for_tests(Feed {
        built_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        age_seconds: Some(60),
        stale: Some(false),
        count: Some(1),
        models: vec![FeedModel {
            id: "gpt-5.9-mini".to_string(),
            aliases: vec!["gpt-5.9-mini-alias".to_string()],
            input_per_mtok: 0.75,
            output_per_mtok: 4.5,
            cached_input_per_mtok: None,
            context_length: None,
            source: Some("openrouter".to_string()),
        }],
    });
    let price = |model| pricing_resolution::resolve_model_pricing(model, AgentKind::Codex);
    let p = price("gpt-5.9-mini").expect("exact feed id");
    assert_eq!((p.input_per_m, p.output_per_m), (0.75, 4.5));
    assert!(price("gpt-5.9-mini-alias").is_some(), "feed alias is an exact match");
    assert!(price("openai/gpt-5.9-mini").is_none(), "no vendor-prefix stripping");
    assert!(price("gpt-5.9-mini-high").is_none(), "no substring match");
    clear_feed_for_tests();
}

#[test]
fn cost_totals_never_count_unknown_as_zero() {
    assert_eq!(format_cost_total(1.5, 0), "$1.50");
    assert_eq!(format_cost_total(1.5, 2), "$1.50 + 2 unknown");
    assert_eq!(format_cost_total(0.0, 3), "unknown (3 tasks)");
    assert_eq!(format_cost_total(0.0, 0), "free");
}

#[test]
fn has_known_price_matches_estimate_cost() {
    let _guard = isolated();
    assert!(has_known_price(Some("gpt-5.6-sol"), AgentKind::Codex));
    assert!(!has_known_price(Some("no-such-model"), AgentKind::Codex));
    assert!(!has_known_price(None, AgentKind::Codex));
    assert!(has_known_price(None, AgentKind::Cursor));
}
