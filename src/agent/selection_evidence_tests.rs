// Regression tests for model evidence, neutral defaults, and custom floors.
// Covers resolved advice and the serialized agent-list contract.
// Deps: advice isolation helpers, team overrides, registry, agent JSON.

use super::super::super::selection_scoring::{
    CandidateContext, NEUTRAL_BASE, model_capability_score, score_breakdown,
};
use super::*;
use crate::agent::classifier::Complexity;
use tempfile::TempDir;

fn testing_advice(difficulty: TaskDifficulty, team: Option<&TeamConfig>) -> AdviceReport {
    advise(
        "add tests",
        declared(difficulty, TaskBudget::Standard),
        Some(TaskCategory::Testing),
        team,
        None,
        0,
        None,
    )
}

#[test]
fn matrix_removal_unknown_agy_testing_is_eligible_and_unrated() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Antigravity]);
    crate::agent_config::save_agent_default_model("agy", Some("unknown-model")).expect("model");
    let report = testing_advice(TaskDifficulty::Moderate, None);
    let agy = find(&report, "agy");
    assert!(agy.eligible, "{:?}", agy.exclusion_codes);
    assert!(agy.exclusion_codes.is_empty());
    assert_eq!(agy.breakdown.base, 6.0);
    assert!(
        report
            .notes
            .iter()
            .any(|note| note == "agy: unrated: no measured or model capability for testing"),
        "{:?}",
        report.notes
    );
}

#[test]
fn matrix_removal_rated_codex_below_floor_is_excluded() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    crate::scores::test_support::seed_live();
    // The captured gpt-4o-mini ECI is the snapshot minimum (126.56 -> 0.0).
    // An evidenced shortfall (base 0 < floor 8 for complex) excludes codex.
    crate::agent_config::save_agent_default_model("codex", Some("openai/gpt-4o-mini")).expect("model");
    let report = advise(
        "refactor",
        declared(TaskDifficulty::Complex, TaskBudget::Standard),
        Some(TaskCategory::Refactoring),
        None,
        None,
        0,
        None,
    );
    let codex = find(&report, "codex");
    assert!(!codex.eligible);
    assert_eq!(codex.exclusion_codes, vec!["below_floor"]);
    assert_eq!(
        codex.exclusion_reason.as_deref(),
        Some("base 0 < floor 8 for complex")
    );
    assert_eq!(codex.breakdown.base, 0.0);
}

#[test]
fn matrix_removal_team_override_wins_over_rated_model() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    crate::scores::test_support::seed_live();
    // Opus has measured model-level ECI; the team override takes precedence.
    crate::agent_config::save_agent_default_model("codex", Some("opus")).expect("model");
    for (score, eligible) in [(10, true), (4, false)] {
        let team: TeamConfig = toml::from_str(&format!(
            "id = 'override'\ndisplay_name = 'Override'\npreferred_agents = []\n\
                 [overrides.codex]\ntesting = {score}\n"
        ))
        .expect("team");
        let report = testing_advice(TaskDifficulty::Complex, Some(&team));
        let codex = find(&report, "codex");
        assert_eq!(codex.eligible, eligible);
        assert_eq!(codex.breakdown.base, f64::from(score));
        assert_eq!(codex.breakdown.model_capability, 0.0);
        // Team override wins over rated model capability; total equals base with no complexity bonus.
        assert_eq!(codex.score, f64::from(score));
        assert!(!report.notes.iter().any(|note| note.starts_with("codex: unrated")));
    }
}

#[test]
fn matrix_removal_custom_floor_keeps_declared_capability_and_strength() {
    let (_temp, _home, _cache) = isolated();
    crate::paths::ensure_dirs().expect("aid dirs");
    let dir = crate::paths::aid_dir().join("agents");
    std::fs::create_dir_all(&dir).expect("agents dir");
    for (strengths, eligible) in [("[]", false), ("['testing']", true)] {
        std::fs::write(
            dir.join("custom-floor.toml"),
            format!(
                "[agent]\nid = 'custom-floor'\ndisplay_name = 'Custom'\ncommand = 'bash'\n\
             strengths = {strengths}\n[agent.capabilities]\ntesting = 3\n"
            ),
        )
        .expect("custom config");
        let report = testing_advice(TaskDifficulty::Moderate, None);
        let custom = report
            .custom_candidates
            .iter()
            .find(|c| c.agent == "custom-floor")
            .expect("custom");
        assert_eq!(custom.category_capability, 3);
        assert_eq!(custom.strength_bonus, if eligible { 5 } else { 0 });
        assert_eq!(custom.eligible, eligible);
        assert_eq!(
            custom.exclusion_codes,
            if eligible { vec![] } else { vec!["below_floor"] }
        );
        if !eligible {
            assert_eq!(
                custom.exclusion_reason.as_deref(),
                Some("base 3 < floor 6 for moderate")
            );
        }
    }
}

#[test]
fn matrix_removal_agent_list_json_has_no_agent_capability_map() {
    let (_temp, _home, _cache) = isolated();
    crate::paths::ensure_dirs().expect("aid dirs");
    let dir = crate::paths::aid_dir().join("agents");
    std::fs::create_dir_all(&dir).expect("agents dir");
    std::fs::write(
        dir.join("custom-json.toml"),
        "[agent]\nid = 'custom-json'\ndisplay_name = 'Custom'\ncommand = 'bash'\n",
    )
    .expect("custom config");
    let store = crate::store::Store::open_memory().expect("store");
    let list = crate::cmd::agent_json::agents_list_value(&store).expect("agent list");
    let agents = list["agents"].as_array().expect("agents");
    assert!(agents.iter().any(|agent| agent["name"] == "custom-json"));
    for agent in agents {
        assert!(
            agent.get("capabilities").is_none(),
            "{} has a capability map",
            agent["name"]
        );
    }
}

#[test]
fn discovered_agy_model_uses_neutral_base_when_capability_is_unknown() {
    let temp = TempDir::new().expect("temp dir");
    let _home = AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("aid dirs");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let cache = serde_json::json!({
        "agy": {"models": ["gemini-3.7-flash-high"], "updated_at_secs": now}
    });
    std::fs::write(
        crate::paths::aid_dir().join("served_models_cache.json"),
        cache.to_string(),
    )
    .expect("served-model cache");

    let capability = model_capability_score(AgentKind::Antigravity, "gemini-3.7-flash-high", TaskCategory::SimpleEdit);
    assert_eq!(capability, None);
    let profile = TaskProfile {
        category: TaskCategory::SimpleEdit,
        complexity: Complexity::Low,
    };
    let history = HashMap::new();
    let costs = HashMap::new();
    let ctx = CandidateContext {
        profile: &profile,
        team: None,
        history_map: &history,
        avg_cost_map: &costs,
        team_default: None,
        budget: false,
        penalize_rate_limit: false,
    };
    assert_eq!(
        score_breakdown(&ctx, AgentKind::Antigravity, Some("gemini-3.7-flash-high")).base,
        NEUTRAL_BASE
    );
}
