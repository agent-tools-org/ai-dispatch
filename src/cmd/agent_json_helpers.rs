// Agent metadata helpers for JSON command and web responses.
// Exports: capability lookup, quota/model helpers, and command checks.
// Deps: agent registry types, model catalog, rate limits, and selection scores.

use std::collections::HashMap;

use crate::agent::classifier::TaskCategory;
use crate::agent::custom::CustomAgentConfig;
use crate::cmd::agent_json_types::{GroupHoldJson, QuotaJson};
use crate::types::AgentKind;

pub fn command_installed(command: &str) -> bool {
    let binary = command.split_whitespace().next().unwrap_or_default();
    if binary.is_empty() {
        return false;
    }
    std::process::Command::new("which")
        .arg(binary)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn get_agent_capabilities(
    kind: AgentKind,
    custom_config: Option<&CustomAgentConfig>,
) -> HashMap<String, i32> {
    let mut caps = HashMap::new();
    if let Some(config) = custom_config {
        caps.insert(TaskCategory::Research.label().to_string(), config.capabilities.research);
        caps.insert(TaskCategory::SimpleEdit.label().to_string(), config.capabilities.simple_edit);
        caps.insert(TaskCategory::ComplexImpl.label().to_string(), config.capabilities.complex_impl);
        caps.insert(TaskCategory::Frontend.label().to_string(), config.capabilities.frontend);
        caps.insert(TaskCategory::Debugging.label().to_string(), config.capabilities.debugging);
        caps.insert(TaskCategory::Testing.label().to_string(), config.capabilities.testing);
        caps.insert(TaskCategory::Refactoring.label().to_string(), config.capabilities.refactoring);
        caps.insert(TaskCategory::Documentation.label().to_string(), config.capabilities.documentation);
    } else {
        for category in &[
            TaskCategory::Research,
            TaskCategory::SimpleEdit,
            TaskCategory::ComplexImpl,
            TaskCategory::Frontend,
            TaskCategory::Debugging,
            TaskCategory::Testing,
            TaskCategory::Refactoring,
            TaskCategory::Documentation,
        ] {
            let score = crate::agent::selection::AGENT_CAPABILITIES.iter()
                .find(|(candidate, _)| *candidate == kind)
                .and_then(|(_, scores)| scores.iter().find(|(candidate, _)| *candidate == *category))
                .map(|(_, score)| *score)
                .unwrap_or(1);
            caps.insert(category.label().to_string(), score);
        }
    }
    caps
}

pub(crate) fn build_quota_json(rlk: &AgentKind, custom_name: Option<&str>) -> QuotaJson {
    if crate::rate_limit::is_rate_limited(rlk, custom_name) {
        let info = crate::rate_limit::get_rate_limit_info(rlk, custom_name);
        return QuotaJson {
            state: "limited".to_string(),
            recovery_at: info.as_ref().and_then(|value| value.recovery_at.clone()),
            message: info.as_ref().and_then(|value| value.message.clone()),
            source: "marker".to_string(),
            groups: vec![],
        };
    }
    let groups = crate::rate_limit::active_group_holds(rlk, custom_name)
        .into_iter()
        .map(|(group, info)| GroupHoldJson {
            group,
            recovery_at: info.recovery_at,
            message: info.message,
        })
        .collect::<Vec<_>>();
    let state = if groups.is_empty() { "ok" } else { "partial" };
    QuotaJson {
        state: state.to_string(),
        recovery_at: None,
        message: None,
        source: "marker".to_string(),
        groups,
    }
}

pub(crate) fn builtin_profile(name: &str) -> Option<AgentKind> {
    AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(name))
}

pub(crate) fn custom_has_endpoint(config: &CustomAgentConfig) -> bool {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|url| !url.is_empty())
}

pub(crate) fn rate_limit_kind(kind: AgentKind, _custom_config: Option<&CustomAgentConfig>) -> AgentKind {
    kind
}

pub(crate) fn catalog_default_model(kind: AgentKind) -> Option<String> {
    let models = crate::model_catalog::models_for_agent(&kind);
    models
        .iter()
        .find(|model| model.description.to_ascii_lowercase().contains("default"))
        .or_else(|| models.first())
        .map(|model| model.model.to_string())
}

pub(crate) fn metering_label(shape: crate::types::MeteringShape) -> String {
    use crate::types::MeteringShape as M;
    match shape {
        M::AccountPool => "account_pool",
        M::PerModelFamily => "per_model_family",
        M::SpendBudget => "spend_budget",
        M::Subscription => "subscription",
        M::None => "none",
        M::Unknown => "unknown",
    }
    .to_string()
}
