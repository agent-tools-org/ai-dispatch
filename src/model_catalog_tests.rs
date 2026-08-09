// Tests for shared budget-to-model selection.
// Deps: super::{budget_model, model_for_task_budget, model_on_budget_preference},
//       crate::types::{AgentKind, TaskBudget}.

use super::{budget_model, model_for_task_budget, model_on_budget_preference};
use crate::types::{AgentKind, TaskBudget};

#[test]
fn budget_cheap_selects_unknown_tier_as_last_resort() {
    // grok's only catalog row is tier "unknown" — unpriced, not ineligible.
    assert_eq!(
        model_for_task_budget(AgentKind::Grok, TaskBudget::Cheap),
        Some("grok-4.5")
    );
    assert_eq!(
        model_for_task_budget(AgentKind::Grok, TaskBudget::Free),
        Some("grok-4.5")
    );
}

#[test]
fn budget_model_agrees_with_task_budget_cheap() {
    // The two former implementations disagreed on grok; one rule now.
    assert_eq!(
        budget_model(&AgentKind::Grok),
        model_for_task_budget(AgentKind::Grok, TaskBudget::Cheap)
    );
    assert_eq!(
        budget_model(&AgentKind::Codex),
        model_for_task_budget(AgentKind::Codex, TaskBudget::Cheap)
    );
    assert_eq!(
        budget_model(&AgentKind::Claude),
        model_for_task_budget(AgentKind::Claude, TaskBudget::Cheap)
    );
}

#[test]
fn budget_preferred_tiers_beat_unknown() {
    // Cursor has a cheap-tier model; unknown must not win when a preferred
    // tier exists.
    let model = model_for_task_budget(AgentKind::Cursor, TaskBudget::Cheap)
        .expect("cursor has a cheap model");
    assert!(model_on_budget_preference(
        AgentKind::Cursor,
        TaskBudget::Cheap,
        model
    ));
}

#[test]
fn unknown_model_is_not_on_budget_cheap_preference() {
    assert!(!model_on_budget_preference(
        AgentKind::Grok,
        TaskBudget::Cheap,
        "grok-4.5"
    ));
}
