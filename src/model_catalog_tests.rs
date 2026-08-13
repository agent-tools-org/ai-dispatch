// Tests for shared budget-to-model selection.
// Deps: super::{budget_model, model_for_task_budget, model_on_budget_preference},
//       crate::types::{AgentKind, TaskBudget}.

use super::{budget_model, model_for_task_budget, model_on_budget_preference};
use crate::types::{AgentKind, TaskBudget};

#[test]
fn grok_budget_selection_is_a_monotonic_golden_table() {
    let expected = [
        (TaskBudget::Free, "grok-4.6"),
        (TaskBudget::Cheap, "grok-4.6"),
        (TaskBudget::Standard, "grok-4.6"),
        (TaskBudget::Premium, "grok-4.6"),
    ];

    for (budget, model) in expected {
        assert_eq!(model_for_task_budget(AgentKind::Grok, budget), Some(model));
    }
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
fn budget_cheap_picks_lowest_price_within_tier() {
    // gemini cheap-tier: flash has higher capability but flash-lite is ~6x
    // cheaper on output. Free/Cheap must prefer lowest total price.
    assert_eq!(
        model_for_task_budget(AgentKind::Gemini, TaskBudget::Cheap),
        Some("flash-lite")
    );
    assert_eq!(
        budget_model(&AgentKind::Gemini),
        Some("flash-lite")
    );
}

#[test]
fn budget_cheap_picks_lowest_price_across_preferred_tiers() {
    // opencode cheap-tier glm-5.2 is ~$2.36; free-tier deepseek is $0.00.
    // Free/Cheap must pool preferred tiers, not short-circuit on cheap.
    assert_eq!(
        model_for_task_budget(AgentKind::OpenCode, TaskBudget::Cheap),
        Some("opencode/deepseek-v4-flash-free")
    );
    assert_eq!(
        budget_model(&AgentKind::OpenCode),
        Some("opencode/deepseek-v4-flash-free")
    );
}
