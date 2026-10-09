// Unit tests for advise exclusion codes and caller-pool verdicts.
// Covers: absent capability evidence, floor shortfall, weaker/demote/clear on the caller's pool.
// Deps: selection_advice_gate, AgentKind, TaskCategory.

use super::*;

fn caller(capability: Option<f64>) -> CallerAdvice {
    CallerAdvice {
        session: "claude-code".to_string(),
        agent: "claude".to_string(),
        provider: "anthropic".to_string(),
        model: capability.map(|_| "opus".to_string()),
        capability,
    }
}

#[test]
fn missing_capability_is_not_excluded_and_noted_unrated() {
    let mut exclusions = Exclusions::default();
    let note = exclusions.floor(None, 6, TaskDifficulty::Moderate, TaskCategory::Frontend);
    assert_eq!(note.as_deref(), Some("unrated: no measured or model capability for frontend"));
    assert!(exclusions.is_empty());
    let (reason, codes) = exclusions.into_parts();
    assert_eq!(reason, None);
    assert!(codes.is_empty());
}

#[test]
fn measured_shortfall_keeps_floor_reason() {
    let mut exclusions = Exclusions::default();
    assert_eq!(exclusions.floor(Some(4.0), 6, TaskDifficulty::Moderate, TaskCategory::Frontend), None);
    exclusions.budget(false, TaskBudget::Free);
    let (reason, codes) = exclusions.into_parts();
    assert_eq!(reason.as_deref(), Some("base 4 < floor 6 for moderate; no model for budget free"));
    assert_eq!(codes, vec!["below_floor", "no_budget_model"]);
}

#[test]
fn same_pool_weaker_model_is_excluded_when_caller_model_known() {
    let known = caller(Some(9.4));
    assert_eq!(pool_verdict(Some(&known), AgentKind::Claude, Some(8.8)), PoolVerdict::Weaker);
    assert_eq!(pool_verdict(Some(&known), AgentKind::Claude, Some(9.4)), PoolVerdict::Clear);
    assert_eq!(pool_verdict(Some(&known), AgentKind::Codex, Some(1.0)), PoolVerdict::Clear);
}

#[test]
fn same_pool_is_demoted_when_caller_model_unknown() {
    let unknown = caller(None);
    assert_eq!(pool_verdict(Some(&unknown), AgentKind::Claude, Some(8.8)), PoolVerdict::Demote);
    assert_eq!(pool_verdict(Some(&unknown), AgentKind::Cursor, Some(8.8)), PoolVerdict::Clear);
    assert_eq!(pool_verdict(None, AgentKind::Claude, Some(1.0)), PoolVerdict::Clear);
    assert!(demotion_reason(&unknown).contains("same pool as caller (anthropic)"));
}

#[test]
fn caller_advice_resolves_pool_and_leaderboard_capability() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::scores::test_support::seed_live();
    let advice = caller_advice("claude-code", Some("opus"), crate::agent::classifier::TaskCategory::ComplexImpl).expect("claude pool");
    assert_eq!(advice.provider, "anthropic");
    assert!(advice.capability.is_some());
    let unknown = caller_advice("codex", Some("no-such-model"), crate::agent::classifier::TaskCategory::ComplexImpl).expect("codex pool");
    assert_eq!(unknown.provider, "openai-chatgpt-plan");
    assert_eq!(unknown.capability, None);
    assert!(caller_advice("terminal", None, crate::agent::classifier::TaskCategory::ComplexImpl).is_none());
}
