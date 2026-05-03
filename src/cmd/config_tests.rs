// Tests for pricing override loading and merged model resolution.
// Exports: module-scoped tests only
// Deps: super::load_pricing_overrides, super::merged_agent_models, crate::paths

use std::collections::HashMap;

use super::config_display::{ModelHistory, recent_observed_models_line};
use super::{load_pricing_overrides, merged_agent_models};
use crate::paths::AidHomeGuard;
use crate::types::AgentKind;

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

#[test]
fn merged_agent_models_include_gemini_3_catalog() {
    let merged = merged_agent_models().unwrap();
    let gemini_models: Vec<_> = merged
        .iter()
        .filter(|model| model.agent == AgentKind::Gemini)
        .map(|model| model.model.as_str())
        .collect();
    for expected in [
        "gemini-3.1-pro-preview",
        "gemini-3-flash-preview",
        "gemini-3-flash-lite-preview",
        "gemini-2.5-flash",
        "gemini-2.5-pro",
    ] {
        assert!(
            gemini_models.iter().any(|name| *name == expected),
            "missing model {expected} in {:?}",
            gemini_models
        );
    }
}

#[test]
fn recent_observed_models_line_lists_top_three_unsorted_extras_only() {
    let mut mh = HashMap::new();
    mh.insert(
        (AgentKind::Gemini, "gemini-3-flash-preview".into()),
        ModelHistory {
            task_count: 1,
            success_rate: 50.0,
            avg_cost: 0.0,
        },
    );
    mh.insert(
        (
            AgentKind::Gemini,
            "actually-observed-gemini-99".into(),
        ),
        ModelHistory {
            task_count: 7,
            success_rate: 100.0,
            avg_cost: 0.1,
        },
    );
    mh.insert(
        (
            AgentKind::Gemini,
            "actually-observed-gemini-98".into(),
        ),
        ModelHistory {
            task_count: 2,
            success_rate: 100.0,
            avg_cost: 0.1,
        },
    );
    mh.insert(
        (
            AgentKind::Gemini,
            "actually-observed-gemini-97".into(),
        ),
        ModelHistory {
            task_count: 5,
            success_rate: 100.0,
            avg_cost: 0.1,
        },
    );
    mh.insert(
        (
            AgentKind::Gemini,
            "actually-observed-fourth".into(),
        ),
        ModelHistory {
            task_count: 1,
            success_rate: 100.0,
            avg_cost: 0.0,
        },
    );

    let line = recent_observed_models_line(AgentKind::Gemini, &mh).expect("recent line present");
    assert!(line.starts_with("  Recent:    "));
    assert!(line.contains("actually-observed-gemini-99 (7)"));
    assert!(line.contains("actually-observed-gemini-97 (5)"));
    assert!(line.contains("actually-observed-gemini-98 (2)"));
    assert!(!line.contains("actually-observed-fourth"));
    assert!(!line.contains("gemini-3-flash-preview"));
}
