// Tests for shared budget-to-model selection.
// Deps: super::{budget_model, model_for_task_budget, model_on_budget_preference, models_for_agent},
//       crate::types::{AgentKind, TaskBudget}.

use super::{budget_model, model_for_task_budget, model_on_budget_preference, models_for_agent};
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
fn models_for_agent_merges_cached_agy_model_as_unknown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
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

    let models = models_for_agent(&AgentKind::Antigravity);
    let discovered = models.iter()
        .find(|model| model.model == "gemini-3.7-flash-high")
        .expect("discovered model");
    assert_eq!(discovered.input_per_m, None);
    assert_eq!(discovered.output_per_m, None);
    assert_eq!(discovered.capability, None);
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
    // grok is unpriced: both rows sit on tier "unknown", which is a last-resort
    // fallback, never a *preferred* tier. Deleting this let a tier reassignment
    // (unknown -> cheap/premium) pass unnoticed and strand TaskBudget::Free.
    assert!(!model_on_budget_preference(
        AgentKind::Grok,
        TaskBudget::Cheap,
        "grok-4.5"
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
