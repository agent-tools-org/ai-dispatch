// Resolved-price regressions at score_breakdown and the actual advise boundary.
// Sticky model pins keep the tested route identical across budget profiles and overrides.
// Deps: existing cost cache seam, synthetic pricing files, thread-local home/quota guards.

use std::collections::HashMap;

use super::selection_scoring::{CandidateContext, ScoreBreakdown, score_breakdown};
use super::{AdviceCandidate, advise};
use crate::agent::classifier::{Complexity, TaskCategory, TaskProfile};
use crate::agent::run_model::RunModelSource;
use crate::cost::clear_feed_for_tests;
use crate::live_quota::CacheDirGuard;
use crate::paths::AidHomeGuard;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};
use tempfile::TempDir;

fn isolated() -> (TempDir, AidHomeGuard, CacheDirGuard) {
    let temp = TempDir::new().expect("temporary pricing home");
    let home = AidHomeGuard::set(temp.path());
    let cache = CacheDirGuard::set(&temp.path().join("quota"));
    crate::paths::ensure_dirs().expect("pricing directories");
    clear_feed_for_tests();
    (temp, home, cache)
}

fn write_override(agent: AgentKind, model: &str, input: f64, output: f64) {
    let body = serde_json::json!({"models": [{
        "agent": agent.as_str(), "model": model, "input_per_m": input,
        "output_per_m": output, "tier": "premium", "description": "Synthetic test price",
        "updated": "2026-10-03"
    }]});
    std::fs::write(crate::paths::pricing_path(), body.to_string()).expect("pricing override");
    clear_feed_for_tests();
}

fn route_score(agent: AgentKind, model: &str, budget: TaskBudget) -> AdviceCandidate {
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![agent]);
    crate::agent_config::save_agent_default_model(agent.as_str(), Some(model)).expect("sticky model");
    let profile = TaskProfile {
        category: TaskCategory::SimpleEdit,
        complexity: Complexity::Low,
    };
    let context = CandidateContext {
        profile: &profile,
        team: None,
        history_map: &HashMap::new(),
        avg_cost_map: &HashMap::new(),
        team_default: None,
        budget: budget.uses_budget_mode(),
        penalize_rate_limit: true,
    };
    let direct = score_breakdown(&context, agent, Some(model));
    let declared = DeclaredTaskProfile {
        difficulty: TaskDifficulty::Simple,
        budget,
        urgency: TaskUrgency::Normal,
        rigor: TaskRigor::Standard,
    };
    let report = advise("rename a field", declared, Some(profile.category), None, None, 0, None);
    let candidate = report
        .candidates
        .into_iter()
        .find(|item| item.agent == agent.as_str())
        .expect("pinned advice candidate");
    assert_eq!(candidate.model.as_deref(), Some(model));
    assert!(candidate.pinned && candidate.installed && candidate.eligible);
    assert_eq!(candidate.source, RunModelSource::Sticky);
    assert_eq!(candidate.breakdown, direct, "{agent:?}/{model}/{budget:?}");
    assert_eq!(candidate.score, direct.total);
    candidate
}

fn without_budget(mut score: ScoreBreakdown) -> ScoreBreakdown {
    score.total -= score.budget_penalty;
    score.budget_penalty = 0.0;
    score
}

fn assert_penalty(agent: AgentKind, model: &str, expected: f64) -> AdviceCandidate {
    let standard = route_score(agent, model, TaskBudget::Standard);
    assert_eq!(standard.breakdown.budget_penalty, 0.0);
    for budget in [TaskBudget::Free, TaskBudget::Cheap, TaskBudget::Premium] {
        let candidate = route_score(agent, model, budget);
        let penalty = if budget.uses_budget_mode() { expected } else { 0.0 };
        assert_eq!(
            candidate.breakdown.budget_penalty, penalty,
            "{agent:?}/{model}/{budget:?}"
        );
        assert_eq!(candidate.score, standard.score + penalty);
        assert_eq!(
            without_budget(candidate.breakdown),
            standard.breakdown,
            "all other score terms remain identical"
        );
        assert_eq!(candidate.exclusion_codes, standard.exclusion_codes);
    }
    standard
}

#[test]
fn resolved_price_catalog_subscription_unknown_and_free_at_both_boundaries() {
    let _home = isolated();
    let row = crate::model_catalog::AGENT_MODELS
        .iter()
        .find(|row| row.agent == AgentKind::Cursor && row.model == "composer-2.5")
        .expect("subscription with API-looking catalog price");
    assert!(row.input_per_m > 0.0 && row.output_per_m > 0.0);
    for (agent, model, penalty) in [
        (AgentKind::Cursor, "composer-2.5", 0.0),
        (AgentKind::Copilot, "synthetic-subscription-model", 0.0),
        (AgentKind::Droid, "synthetic-unknown-model", 0.0),
        (AgentKind::Codex, "gpt-5.6-sol", -3.0),
        (AgentKind::OpenCode, "opencode/deepseek-v4-flash-free", 0.0),
    ] {
        assert_penalty(agent, model, penalty);
    }
    clear_feed_for_tests();
}

#[test]
fn resolved_price_zero_override_removes_catalog_paid_penalty() {
    let _home = isolated();
    for (agent, model, before_penalty) in [
        (AgentKind::Codex, "gpt-5.6-sol", -3.0),
        (AgentKind::Cursor, "composer-2.5", 0.0),
        (AgentKind::Droid, "synthetic-unknown-model", 0.0),
    ] {
        let before = assert_penalty(agent, model, before_penalty);
        write_override(agent, model, 0.0, 0.0);
        let after = assert_penalty(agent, model, 0.0);
        assert_eq!(
            after, before,
            "pricing changes neither non-budget scores nor eligibility"
        );
    }
    clear_feed_for_tests();
}

#[test]
fn resolved_price_positive_exact_override_prices_free_subscription_and_unknown() {
    let _home = isolated();
    for (agent, model) in [
        (AgentKind::OpenCode, "opencode/deepseek-v4-flash-free"),
        (AgentKind::Cursor, "composer-2.5"),
        (AgentKind::Droid, "synthetic-unknown-model"),
    ] {
        if crate::paths::pricing_path().exists() {
            std::fs::remove_file(crate::paths::pricing_path()).expect("reset synthetic override");
        }
        clear_feed_for_tests();
        let before = assert_penalty(agent, model, 0.0);
        for (input, output) in [(2.0, 0.0), (0.0, 4.0)] {
            write_override(agent, &model.to_uppercase(), input, output);
            let after = assert_penalty(agent, model, -3.0);
            assert_eq!(after, before, "override preserves all non-budget terms and eligibility");
            assert_penalty(agent, &format!("{model}-neighbor"), 0.0);
            assert_penalty(AgentKind::Oz, model, 0.0);
        }
    }
    clear_feed_for_tests();
}

#[test]
fn resolved_price_vendor_exact_feed_is_paid_only_on_its_vendor() {
    let _home = isolated();
    let model = "claude-synthetic-price-model";
    let vendor_before = assert_penalty(AgentKind::Claude, model, 0.0);
    let reseller_before = assert_penalty(AgentKind::Droid, model, 0.0);
    for (input, output) in [(3.0, 0.0), (0.0, 15.0)] {
        let feed = serde_json::json!({
            "built_at": chrono::Utc::now().to_rfc3339(), "age_seconds": 1,
            "stale": false, "count": 1,
            "models": [{"id": model, "input_per_mtok": input, "output_per_mtok": output,
                "cached_input_per_mtok": null, "context_length": null, "source": null}]
        });
        std::fs::write(crate::paths::aid_dir().join("prices.json"), feed.to_string()).expect("feed");
        clear_feed_for_tests();
        let price = crate::cost::resolve_pricing(Some(model), AgentKind::Claude).expect("exact feed");
        assert_eq!((price.input_per_m, price.output_per_m), (input, output));
        assert!(crate::cost::resolve_pricing(Some(model), AgentKind::Droid).is_none());
        assert_eq!(assert_penalty(AgentKind::Claude, model, -3.0), vendor_before);
        assert_eq!(assert_penalty(AgentKind::Droid, model, 0.0), reseller_before);
        assert_penalty(AgentKind::Claude, &format!("{model}-neighbor"), 0.0);
    }
    clear_feed_for_tests();
}
