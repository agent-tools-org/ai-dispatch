// Declared-budget warning for a resolved `aid run` / `aid batch` model.
// Exports: declared_budget_warning.
// Deps: model_catalog, run_model::RunModel, AgentKind, TaskBudget.

use crate::agent::run_model::{RunModel, RunModelSource};
use crate::types::{AgentKind, TaskBudget};

/// Warning when a free/cheap declared budget did not get a budget-preferred
/// model. Silent for `--model` and self-heal retries: the caller chose.
pub(crate) fn declared_budget_warning(
    agent_name: &str,
    declared_budget: Option<TaskBudget>,
    run_model: &RunModel,
) -> Option<String> {
    if matches!(run_model.source, RunModelSource::Explicit | RunModelSource::ForcedDefault) {
        return None;
    }
    declared_budget_model_warning(
        agent_name,
        declared_budget,
        run_model.model.as_deref(),
        run_model.source == RunModelSource::Sticky,
    )
}

/// Warning text when a free/cheap declared budget is not the model actually chosen.
fn declared_budget_model_warning(
    agent_name: &str,
    declared_budget: Option<TaskBudget>,
    chosen_model: Option<&str>,
    from_config: bool,
) -> Option<String> {
    let budget = declared_budget?;
    if !matches!(budget, TaskBudget::Free | TaskBudget::Cheap) {
        return None;
    }
    let kind = AgentKind::parse_str(agent_name);
    let on_preference = chosen_model.is_some_and(|name| {
        kind.is_some_and(|kind| crate::model_catalog::model_on_budget_preference(kind, budget, name))
    });
    if on_preference {
        return None;
    }
    let catalog_has_preference = kind.is_some_and(|kind| {
        crate::model_catalog::model_for_task_budget(kind, budget)
            .is_some_and(|name| crate::model_catalog::model_on_budget_preference(kind, budget, name))
    });
    let chosen = chosen_model.unwrap_or("agent default");
    if from_config && catalog_has_preference {
        return Some(format!(
            "[aid] Warning: configured default model '{chosen}' overrides declared budget {}",
            budget.label()
        ));
    }
    Some(format!(
        "[aid] Warning: agent '{agent_name}' has no model eligible for declared budget {}; using {chosen}",
        budget.label()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_override_warning_does_not_claim_no_eligible_model() {
        let msg = declared_budget_model_warning(
            "gemini",
            Some(TaskBudget::Cheap),
            Some("pro"),
            true,
        )
        .expect("configured default that outranks cheap catalog rows must warn");
        assert!(
            !msg.contains("no model eligible"),
            "catalog still has cheap rows; they were outranked: {msg}"
        );
        assert!(
            msg.contains("configured default") && msg.contains("pro") && msg.contains("cheap"),
            "{msg}"
        );
    }

    #[test]
    fn missing_catalog_tier_still_reports_no_eligible_model() {
        let msg = declared_budget_model_warning(
            "claude",
            Some(TaskBudget::Free),
            Some("claude-opus-5"),
            false,
        )
        .expect("claude has no free catalog row");
        assert!(msg.contains("no model eligible"), "{msg}");
        assert!(msg.contains("free"), "{msg}");
    }

    #[test]
    fn on_preference_model_does_not_warn() {
        assert_eq!(
            declared_budget_model_warning(
                "gemini",
                Some(TaskBudget::Cheap),
                Some("flash-lite"),
                false,
            ),
            None
        );
    }
}
