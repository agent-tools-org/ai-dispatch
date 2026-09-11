// Final dispatch model attribution after quota routing and served-model validation.
// Exports: model_selection_info; deps: RunArgs, agent_config, model_catalog.

use super::RunArgs;
use crate::agent::model_validation::ModelSource;
use crate::types::{AgentKind, TaskBudget};

pub(super) fn model_selection_info(args: &RunArgs, effective_model: Option<&str>) -> String {
    let adapter_default = if effective_model.is_none() {
        match AgentKind::parse_str(&args.agent_name) {
            Some(AgentKind::Cursor) => Some("composer-2.5".to_string()),
            Some(AgentKind::Qwen) => Some(crate::model_catalog::get_qwen_selected_model()
                .unwrap_or_else(|| "coder-model".to_string())),
            Some(AgentKind::MiMoCode) => Some("mimo/mimo-auto".to_string()),
            _ => None,
        }
    } else {
        None
    };
    let source = match effective_model {
        None if adapter_default.is_some() => "adapter default (no caller -m)",
        None => "CLI default (no -m)",
        Some(model) if args.model_source == ModelSource::UserSupplied
            && args.model.as_deref() == Some(model) => "--model",
        Some(model) if crate::agent_config::get_default_model(&args.agent_name).as_deref()
            == Some(model) => "agent config",
        Some(model) if matches!(args.declared_budget, Some(TaskBudget::Free | TaskBudget::Cheap))
            && AgentKind::parse_str(&args.agent_name).is_some_and(|kind| {
                args.declared_budget.and_then(|budget| {
                    crate::model_catalog::model_for_task_budget(kind, budget)
                }) == Some(model)
            }) => "catalog (declared budget)",
        Some(_) => "quota/budget routing",
    };
    format!(
        "[aid] {} model: {}; source: {source}",
        args.agent_name,
        effective_model.or(adapter_default.as_deref()).unwrap_or("CLI default (no -m)")
    )
}
