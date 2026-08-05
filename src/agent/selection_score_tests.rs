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

    assert_eq!(score.to_bits(), 0x4030_4ccc_cccc_cccd);
    assert_eq!(breakdown.total.to_bits(), score.to_bits());
}
