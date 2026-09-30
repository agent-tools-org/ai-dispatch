// Every displayed model price (config pricing, agent JSON, config agent profile) is cost::resolve_pricing.
// Exports: module-scoped tests only.
// Deps: config pricing table, config_display::render_models_line, agent_json list, cost pricing.

use std::collections::HashMap;

use super::config_display::render_models_line;
use super::pricing_table;
use crate::cost::{clear_feed_for_tests, estimate_cost, has_known_price, resolve_pricing};
use crate::model_catalog::AGENT_MODELS;
use crate::types::AgentKind;

/// One route's price as each display shows it.
struct Shown {
    table: (String, String),
    json: (Option<f64>, Option<f64>),
    profile: Option<String>,
}

fn usd(value: Option<f64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |v| format!("${v:.2}"))
}

/// The config pricing table and agent JSON list, rendered once per scenario.
struct Displays {
    table: String,
    agents: crate::cmd::agent_json_types::AgentListJson,
}

fn displays() -> Displays {
    let store = crate::store::Store::open_memory().expect("store");
    Displays {
        table: pricing_table().expect("pricing table"),
        agents: crate::cmd::agent_json::get_agents_list_with_installed(&store, &[]).expect("agent list"),
    }
}

fn shown(displays: &Displays, agent: AgentKind, model: &str) -> Shown {
    let cells: Vec<&str> = displays.table
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .find(|cells| cells.len() > 4 && cells[0] == agent.as_str() && cells[1] == model)
        .unwrap_or_else(|| panic!("{agent:?}/{model} missing from config pricing"));
    let row = displays.agents.agents.iter().find(|a| a.name == agent.as_str()).expect("agent")
        .models.available.iter().find(|m| m.model == model)
        .unwrap_or_else(|| panic!("{agent:?}/{model} missing from agent JSON"));
    let profile = render_models_line(agent, &HashMap::new()).lines()
        .find(|line| line.split_whitespace().next() == Some(model))
        .map(|line| line.split(", ").nth(2).expect("price cell").split(')').next().expect("price").to_string());
    Shown {
        table: (cells[3].to_string(), cells[4].to_string()),
        json: (row.input_per_m, row.output_per_m),
        profile,
    }
}

fn assert_displays_resolved_price(displays: &Displays, agent: AgentKind, model: &str) -> Shown {
    let resolved = resolve_pricing(Some(model), agent);
    let (input, output) = (resolved.map(|p| p.input_per_m), resolved.map(|p| p.output_per_m));
    let shown = shown(displays, agent, model);
    assert_eq!(shown.table, (usd(input), usd(output)), "{agent:?}/{model} config pricing");
    assert_eq!(shown.json, (input, output), "{agent:?}/{model} agent JSON");
    assert_eq!(shown.json.0.is_some(), has_known_price(Some(model), agent), "{agent:?}/{model}");
    if let Some(profile) = &shown.profile {
        let expected = resolved.map_or("unknown".to_string(), |p| format!("${:.2}/${:.2}/M", p.input_per_m, p.output_per_m));
        assert_eq!(profile, &expected, "{agent:?}/{model} config agent profile");
    }
    if let (Some(input), Some(output)) = shown.json {
        let cost = estimate_cost(1_000_000, Some(model), agent).expect("known price has a cost");
        assert!((cost - (0.7 * input + 0.3 * output)).abs() < 1e-9, "{agent:?}/{model}: {cost}");
    }
    shown
}

fn isolated() -> (tempfile::TempDir, crate::paths::AidHomeGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = crate::paths::AidHomeGuard::set(temp.path());
    clear_feed_for_tests();
    (temp, guard)
}

#[test]
fn every_catalog_row_displays_the_resolved_price() {
    let _home = isolated();
    let displays = displays();
    for row in AGENT_MODELS {
        let shown = assert_displays_resolved_price(&displays, row.agent, row.model);
        assert!(shown.profile.is_some(), "{:?}/{} missing from config agent profile", row.agent, row.model);
    }
    let table = |agent, model| shown(&displays, agent, model).table;
    let unknown = ("unknown".to_string(), "unknown".to_string());
    let included = ("$0.00".to_string(), "$0.00".to_string());
    assert_eq!(table(AgentKind::Droid, "claude-opus-5"), unknown);
    assert_eq!(table(AgentKind::Oz, "auto"), unknown);
    assert_eq!(table(AgentKind::Grok, "grok-4.6"), unknown);
    assert_eq!(table(AgentKind::Cursor, "composer-2.5"), included, "subscription beats catalog");
}

#[test]
fn pricing_override_is_what_every_display_shows() {
    let _home = isolated();
    crate::paths::ensure_dirs().expect("aid dirs");
    let rows = [("codex", "gpt-5.6-sol", 1.0, 2.0), ("cursor", "composer-2.5", 5.0, 5.0), ("codex", "gpt-9-uncatalogued", 3.0, 4.0)];
    let models: Vec<_> = rows.iter().map(|(agent, model, input, output)| serde_json::json!({
        "agent": agent, "model": model, "input_per_m": input, "output_per_m": output,
        "tier": "premium", "description": "operator price", "updated": "2026-09-30"
    })).collect();
    std::fs::write(crate::paths::pricing_path(), serde_json::json!({ "models": models }).to_string())
        .expect("pricing overrides");
    clear_feed_for_tests();
    let displays = displays();
    for (agent, model, input, output) in rows {
        let agent = AgentKind::parse_str(agent).expect("agent");
        let shown = assert_displays_resolved_price(&displays, agent, model);
        assert_eq!(shown.json, (Some(input), Some(output)), "{agent:?}/{model}");
        assert_eq!(shown.table, (usd(Some(input)), usd(Some(output))), "{agent:?}/{model}");
        let catalogued = AGENT_MODELS.iter().any(|row| row.agent == agent && row.model == model);
        assert_eq!(shown.profile.is_some(), catalogued, "{agent:?}/{model}");
    }
    clear_feed_for_tests();
}
