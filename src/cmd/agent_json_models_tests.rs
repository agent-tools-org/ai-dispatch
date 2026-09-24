// Tests agent-list model views: served-only rows are unrated, default sources are reported.
// Exports: module-scoped tests only.
// Deps: agent_json builders, isolated AID and codex homes, served-model disk cache.

use super::get_agents_list;
use crate::agent::codex::cli_config::set_test_codex_home;
use crate::cmd::agent_json_types::AgentJson;

fn write_served_cache(entries: serde_json::Value) {
    crate::paths::ensure_dirs().expect("aid dirs");
    crate::agent::model_validation::clear_served_models_cache();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let mut map = serde_json::Map::new();
    for (agent, models) in entries.as_object().expect("object") {
        map.insert(
            agent.clone(),
            serde_json::json!({"models": models, "updated_at_secs": now}),
        );
    }
    std::fs::write(
        crate::paths::aid_dir().join("served_models_cache.json"),
        serde_json::Value::Object(map).to_string(),
    )
    .expect("served-model cache");
}

fn agent(name: &str) -> AgentJson {
    let store = crate::store::Store::open_memory().expect("store");
    let list = get_agents_list(&store).expect("agent list");
    list.agents
        .into_iter()
        .find(|agent| agent.name == name)
        .expect("agent")
}

#[test]
fn served_only_models_appear_unrated_for_every_probe_agent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    write_served_cache(serde_json::json!({
        "codex": ["gpt-5.6-sol", "gpt-6-sol"],
        "grok": ["grok-9-served"],
        "cursor": ["cursor-served-9"],
    }));
    let store = crate::store::Store::open_memory().expect("store");
    let list = crate::cmd::agent_json::agents_list_value(&store).expect("agent list");
    let find = |agent: &str, model: &str| {
        list["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .find(|a| a["name"] == agent)
            .expect("agent")["models"]["available"]
            .as_array()
            .expect("available")
            .iter()
            .find(|m| m["model"] == model)
            .cloned()
    };
    for (agent, model) in [
        ("codex", "gpt-6-sol"),
        ("grok", "grok-9-served"),
        ("cursor", "cursor-served-9"),
    ] {
        let row = find(agent, model).unwrap_or_else(|| panic!("{agent}/{model} missing"));
        assert!(row["capability"].is_null(), "{row}");
        assert!(
            row["input_per_m"].is_null() && row["output_per_m"].is_null(),
            "{row}"
        );
        assert_eq!(row["rated"], false, "{row}");
        assert_eq!(row["source"], "served", "{row}");
    }
    let catalog = find("codex", "gpt-5.6-sol").expect("catalog row");
    assert_eq!(catalog["rated"], true);
    assert_eq!(catalog["source"], "catalog");
    let codex_rows = list["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|a| a["name"] == "codex")
        .expect("codex")["models"]["available"]
        .as_array()
        .expect("available")
        .iter()
        .filter(|m| m["model"] == "gpt-5.6-sol")
        .count();
    assert_eq!(
        codex_rows, 1,
        "a served model with a catalog row is not duplicated"
    );
}

#[test]
fn codex_cli_config_default_wins_over_catalog_without_sticky_model() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let codex_home = temp.path().join("codex-home");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    set_test_codex_home(Some(codex_home.clone()));

    let catalog = agent("codex");
    assert_eq!(catalog.models.default_source.as_deref(), Some("catalog"));
    assert_eq!(catalog.models.default.as_deref(), Some("gpt-5.6-sol"));

    std::fs::write(codex_home.join("config.toml"), "model = \"gpt-6-sol\"\n").expect("config");
    let configured = agent("codex");
    assert_eq!(configured.models.default.as_deref(), Some("gpt-6-sol"));
    assert_eq!(
        configured.models.default_source.as_deref(),
        Some("cli_config")
    );

    crate::agent_config::save_agent_default_model("codex", Some("gpt-5.6-luna")).expect("sticky");
    let sticky = agent("codex");
    set_test_codex_home(None);
    assert_eq!(sticky.models.default.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(sticky.models.default_source.as_deref(), Some("sticky"));
}
