// Leaderboard regressions using the captured relay sample and isolated caches.
// Covers the production scoring boundary, category rules, and CLI matching.
// Deps: selection_scoring, price-feed aliases, AidHomeGuard.

use super::selection_scoring::model_capability_score;
use crate::types::AgentKind;

#[test]
fn leaderboard_rule_uses_epoch_eci_instead_of_catalog_rating() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::scores::test_support::seed();
    assert_eq!(
        model_capability_score(
            AgentKind::OpenCode,
            "302ai/kimi-k2-thinking",
            crate::agent::classifier::TaskCategory::Research
        ),
        Some(10.0)
    );
}

#[test]
fn leaderboard_alias_resolution_is_shared_and_unmapped_models_are_unrated() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::scores::test_support::seed_live();
    use crate::agent::classifier::TaskCategory;
    assert_eq!(
        model_capability_score(AgentKind::Claude, "opus", TaskCategory::Research),
        Some(10.0)
    );
    assert_eq!(
        model_capability_score(AgentKind::Droid, "opus", TaskCategory::Research),
        Some(10.0)
    );
    assert_eq!(
        model_capability_score(AgentKind::Claude, "opus-extra", TaskCategory::Research),
        None
    );
    assert_eq!(
        model_capability_score(AgentKind::Codex, "Baichuan 2-7B", TaskCategory::Research),
        None
    );
}

#[test]
fn leaderboard_complexity_has_no_self_assigned_bonus() {
    use super::selection_scoring::{CandidateContext, score_breakdown};
    use crate::agent::classifier::{Complexity, TaskCategory, TaskProfile};
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    let high = TaskProfile {
        category: TaskCategory::ComplexImpl,
        complexity: Complexity::High,
    };
    let low = TaskProfile {
        category: TaskCategory::ComplexImpl,
        complexity: Complexity::Low,
    };
    let empty = std::collections::HashMap::new();
    let costs = std::collections::HashMap::new();
    let mut ctx = CandidateContext {
        profile: &high,
        team: None,
        history_map: &empty,
        avg_cost_map: &costs,
        team_default: None,
        budget: false,
        penalize_rate_limit: false,
    };
    for kind in crate::agent::routable_builtins() {
        ctx.profile = &high;
        let high_score = score_breakdown(&ctx, kind, None);
        ctx.profile = &low;
        let low_score = score_breakdown(&ctx, kind, None);
        assert_eq!(high_score, low_score, "{kind:?}");
        assert!(
            serde_json::to_value(high_score)
                .expect("JSON")
                .get("complexity_bonus")
                .is_none()
        );
    }
}

#[test]
fn leaderboard_uses_configured_codex_effort_for_exact_harness() {
    use crate::agent::classifier::TaskCategory;
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::scores::test_support::seed_live();
    let codex = home.path().join("codex");
    std::fs::create_dir_all(&codex).expect("codex home");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex.clone()));
    std::fs::write(
        codex.join("config.toml"),
        "model_reasoning_effort = 'max'\n",
    )
    .expect("config");
    let measured = crate::scores::evidence(
        AgentKind::Codex,
        Some("openai/gpt-5.6-sol"),
        TaskCategory::Testing,
    );
    std::fs::write(
        codex.join("config.toml"),
        "model_reasoning_effort = 'low'\n",
    )
    .expect("config");
    let fallback = crate::scores::evidence(
        AgentKind::Codex,
        Some("openai/gpt-5.6-sol"),
        TaskCategory::Testing,
    );
    crate::agent::codex::cli_config::set_test_codex_home(None);
    assert!((measured.capability.expect("accuracy") - 3.727).abs() < 1e-10);
    assert_eq!(measured.harness, "measured");
    let expected = 10.0 * (161.66 - 126.56) / (167.33 - 126.56);
    assert!((fallback.capability.expect("ECI") - expected).abs() < 1e-10);
    assert_eq!(fallback.harness, "harness unmeasured");
}
