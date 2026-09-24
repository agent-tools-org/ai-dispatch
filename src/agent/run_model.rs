// The one model-resolution function shared by `aid run` and `aid advise`.
// Exports: RunModel, RunModelSource, RunModelInput, resolve_run_model, model_label.
// Deps: agent_config (sticky), custom agent config, model_catalog, codex/qwen CLI config, rate_limit.

use serde::{Deserialize, Serialize};

use crate::agent::custom::CustomAgentConfig;
use crate::config::SelectionConfig;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RunModel {
    /// The model that will run, when aid can know it.
    pub model: Option<String>,
    /// Whether aid passes `model` to the agent CLI (false = the CLI uses its own default).
    pub pinned: bool,
    pub source: RunModelSource,
}

/// Where the model came from. Declaration order is precedence, except that a
/// self-heal retry (`ForcedDefault`) also discards `--model`: the retry exists
/// because the requested model was unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunModelSource {
    Explicit,
    ForcedDefault,
    Sticky,
    CustomForced,
    SmartRoute,
    BudgetRoute,
    CliConfig,
    AgentDefault,
}

impl RunModelSource {
    /// The serialized (snake_case) name.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::ForcedDefault => "forced_default",
            Self::Sticky => "sticky",
            Self::CustomForced => "custom_forced",
            Self::SmartRoute => "smart_route",
            Self::BudgetRoute => "budget_route",
            Self::CliConfig => "cli_config",
            Self::AgentDefault => "agent_default",
        }
    }

    /// Human source text for dispatch logs.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Explicit => "--model",
            Self::ForcedDefault => "self-heal retry",
            Self::Sticky => "agent config",
            Self::CustomForced => "custom agent forced_model",
            Self::SmartRoute => "smart route (declared simple task)",
            Self::BudgetRoute => "budget route",
            Self::CliConfig => "CLI config (no -m)",
            Self::AgentDefault => "agent default",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RunModelInput<'a> {
    /// The routed name: a builtin id or a custom agent id (sticky config key).
    pub agent_name: &'a str,
    pub kind: AgentKind,
    pub explicit_model: Option<&'a str>,
    /// Self-heal retry: pass nothing so the CLI runs its own default.
    pub force_default: bool,
    pub custom: Option<&'a CustomAgentConfig>,
    /// `--budget`, a declared free/cheap budget, near-limit auto budget, or `selection.budget_mode`.
    pub budget_mode: bool,
    pub declared_budget: Option<TaskBudget>,
    pub declared_difficulty: Option<TaskDifficulty>,
    /// `selection.smart_routing`.
    pub smart_routing: bool,
}

impl<'a> RunModelInput<'a> {
    /// The input `aid advise` and a profile-declared `aid run` share.
    pub(crate) fn declared(
        agent_name: &'a str, kind: AgentKind, custom: Option<&'a CustomAgentConfig>,
        declared: DeclaredTaskProfile, selection: &SelectionConfig,
    ) -> Self {
        Self {
            agent_name, kind, explicit_model: None, force_default: false, custom,
            budget_mode: declared.budget.uses_budget_mode() || selection.budget_mode,
            declared_budget: Some(declared.budget),
            declared_difficulty: Some(declared.difficulty),
            smart_routing: selection.smart_routing,
        }
    }
}

pub(crate) fn resolve_run_model(input: &RunModelInput) -> RunModel {
    let pinned = |model: String, source| RunModel { model: Some(model), pinned: true, source };
    if input.force_default {
        return RunModel { model: None, pinned: false, source: RunModelSource::ForcedDefault };
    }
    if let Some(model) = input.explicit_model {
        return pinned(model.to_string(), RunModelSource::Explicit);
    }
    if let Some(model) = crate::agent_config::get_default_model(input.agent_name) {
        return pinned(model, RunModelSource::Sticky);
    }
    if let Some(custom) = input.custom {
        return RunModel {
            model: custom.forced_model.clone(),
            pinned: false,
            source: if custom.forced_model.is_some() {
                RunModelSource::CustomForced
            } else {
                RunModelSource::AgentDefault
            },
        };
    }
    if let Some(model) = declared_budget_model(input.kind, input.declared_budget) {
        return pinned(model.to_string(), RunModelSource::BudgetRoute);
    }
    if smart_route_applies(input)
        && let Some(model) = crate::model_catalog::budget_model(&input.kind)
    {
        return pinned(model.to_string(), RunModelSource::SmartRoute);
    }
    if input.budget_mode
        && let Some(model) = crate::model_catalog::budget_model(&input.kind)
    {
        return pinned(model.to_string(), RunModelSource::BudgetRoute);
    }
    match cli_configured_default(input.kind) {
        Some(model) => RunModel { model: Some(model), pinned: false, source: RunModelSource::CliConfig },
        None => RunModel { model: None, pinned: false, source: RunModelSource::AgentDefault },
    }
}

/// A declared free/cheap budget pins the catalog budget model.
fn declared_budget_model(kind: AgentKind, budget: Option<TaskBudget>) -> Option<&'static str> {
    let budget = budget.filter(|budget| budget.uses_budget_mode())?;
    crate::model_catalog::model_for_task_budget(kind, budget)
}

/// Cheap routing for a declared trivial/simple task with no declared budget.
fn smart_route_applies(input: &RunModelInput) -> bool {
    !input.budget_mode
        && input.smart_routing
        && input.declared_budget.is_none()
        && matches!(input.declared_difficulty, Some(TaskDifficulty::Trivial | TaskDifficulty::Simple))
        && !crate::rate_limit::is_rate_limited(&input.kind, None)
}

/// The agent CLI's own configured default, where aid can read it.
fn cli_configured_default(kind: AgentKind) -> Option<String> {
    match kind {
        AgentKind::Codex => crate::agent::codex::cli_config::configured_model(),
        AgentKind::Qwen => crate::model_catalog::get_qwen_selected_model(),
        _ => None,
    }
}

/// Human text for a resolved model: `gpt-6-sol (cli_config, not pinned)` or
/// `agent default (unknown)` when aid cannot know the model.
pub(crate) fn model_label(model: Option<&str>, pinned: bool, source: RunModelSource) -> String {
    let Some(model) = model else { return "agent default (unknown)".to_string() };
    let pin = if pinned { "pinned" } else { "not pinned" };
    format!("{model} ({}, {pin})", source.as_str())
}

#[cfg(test)]
#[path = "run_model_tests.rs"]
mod tests;
