// Sticky model resolution for explicit-agent dispatch (`aid run` and `aid batch`).
// Exports: resolve_explicit_agent_model.
// Deps: agent_config, model_catalog, AgentKind, TaskBudget.

use crate::types::{AgentKind, TaskBudget};

/// Precedence: caller `--model` > `agent_config` default > catalog-by-declared-budget.
/// Returns `None` when the caller already supplied `--model`, so that value stays in place.
/// `catalog_if_undeclared` is true for `aid run` (Standard catalog fallback) and false
/// for `aid batch` (catalog only when the task declares a budget).
pub(crate) fn resolve_explicit_agent_model(
    agent_name: &str,
    caller_model: Option<&str>,
    declared_budget: Option<TaskBudget>,
    catalog_if_undeclared: bool,
) -> Option<String> {
    if caller_model.is_some() {
        return None;
    }
    let config = crate::agent_config::get_default_model(agent_name);
    let catalog = catalog_model(agent_name, declared_budget, catalog_if_undeclared);
    let selected = config.clone().or(catalog);
    if let Some(msg) = declared_budget_model_warning(
        agent_name,
        declared_budget,
        selected.as_deref(),
        config.is_some(),
    ) {
        aid_warn!("{msg}");
    }
    selected
}

fn catalog_model(
    agent_name: &str,
    declared_budget: Option<TaskBudget>,
    catalog_if_undeclared: bool,
) -> Option<String> {
    let budget = match declared_budget {
        Some(budget) => budget,
        None if catalog_if_undeclared => TaskBudget::Standard,
        None => return None,
    };
    AgentKind::parse_str(agent_name)
        .and_then(|kind| crate::model_catalog::model_for_task_budget(kind, budget))
        .map(str::to_string)
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
    use crate::paths::AidHomeGuard;

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

    #[test]
    fn resolve_skips_catalog_when_caller_supplied_model() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(temp.path());
        crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("save");
        assert_eq!(
            resolve_explicit_agent_model("gemini", Some("flash"), Some(TaskBudget::Cheap), true),
            None
        );
    }
}
