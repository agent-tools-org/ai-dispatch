// Tests for pricing override loading and merged model resolution.
// Exports: module-scoped tests only
// Deps: super::load_pricing_overrides, super::merged_agent_models, crate::paths

use super::{load_pricing_overrides, merged_agent_models};
use crate::paths::AidHomeGuard;

#[test]
fn loads_and_merges_pricing_overrides() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    std::fs::write(
        crate::paths::pricing_path(),
        r#"{
            "models": [
                {
                    "agent": "codex",
                    "model": "gpt-4.1",
                    "input_per_m": 9.0,
                    "output_per_m": 19.0,
                    "tier": "custom",
                    "description": "override",
                    "updated": "2026-03-17"
                },
                {
                    "agent": "codex",
                    "model": "new-model",
                    "input_per_m": 1.5,
                    "output_per_m": 2.5,
                    "tier": "cheap",
                    "description": "new entry",
                    "updated": "2026-03-17"
                }
            ]
        }"#,
    )
    .unwrap();

    let loaded = load_pricing_overrides().unwrap();
    assert_eq!(loaded.len(), 2);

    let merged = merged_agent_models().unwrap();
    let existing = merged
        .iter()
        .find(|model| model.agent == crate::types::AgentKind::Codex && model.model == "gpt-4.1")
        .unwrap();
    assert_eq!(existing.input_per_m, 9.0);
    assert_eq!(existing.output_per_m, 19.0);
    assert_eq!(existing.tier, "custom");

    let added = merged
        .iter()
        .find(|model| model.agent == crate::types::AgentKind::Codex && model.model == "new-model")
        .unwrap();
    assert_eq!(added.input_per_m, 1.5);
    assert_eq!(added.output_per_m, 2.5);
}
