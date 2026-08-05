// Regression coverage for behavior-preserving selector score decomposition.
// Proves score_for retains the pre-refactor floating-point bit pattern.
// Deps: selection scoring, classifier profile, team config, isolated AID_HOME.

use std::collections::HashMap;

use tempfile::TempDir;

use super::selection_scoring::{CandidateContext, score_breakdown, score_for};
use crate::agent::classifier::{Complexity, TaskCategory, TaskProfile};
use crate::paths::AidHomeGuard;
use crate::team::TeamConfig;
use crate::types::AgentKind;

#[test]
fn score_for_is_bit_identical_to_pre_breakdown_value() {
    let temp = TempDir::new().expect("temp dir");
    let _guard = AidHomeGuard::set(temp.path());
    let profile = TaskProfile {
        category: TaskCategory::ComplexImpl,
        complexity: Complexity::High,
    };
    let history_map = HashMap::from([(AgentKind::Codex, (0.9, 10))]);
    let avg_cost_map = HashMap::new();
    let team = TeamConfig {
        id: "regression".to_string(),
        display_name: "Regression".to_string(),
        description: String::new(),
        preferred_agents: vec!["codex".to_string()],
        default_agent: None,
        overrides: HashMap::new(),
        rules: Vec::new(),
        toolbox: Default::default(),
    };
    let context = CandidateContext {
        profile: &profile,
        team: Some(&team),
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default: None,
        budget: false,
        declared_budget: None,
        penalize_rate_limit: true,
    };

    let score = score_for(&context, AgentKind::Codex);
    let breakdown = score_breakdown(&context, AgentKind::Codex);

    // Absolute pin, so an unintended scoring change cannot slip through: floating
    // addition is not associative and a reordered sum can flip a tie silently.
    // It is derived from the model catalog, so a legitimate catalog refresh moves
    // it — re-pin deliberately and say why. 2026-08-05: 16.3 -> 16.35 when the
    // refresh made gpt-5.6-sol codex's default.
    assert_eq!(score.to_bits(), 0x4030_5999_9999_999a);
    assert_eq!(breakdown.total.to_bits(), score.to_bits());
}
