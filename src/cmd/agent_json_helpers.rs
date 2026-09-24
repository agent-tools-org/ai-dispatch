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
    let avail = crate::route_availability::availability(rlk, custom_name);
    let quota = crate::agent::selection::quota_from(&avail);
    let window = avail
        .probe
        .as_ref()
        .and_then(|probe| crate::agent::selection::tightest_window(&probe.windows))
        .map(|item| item.label.clone())
        .filter(|label| !label.is_empty());
    let held = avail.status == crate::route_availability::RouteStatus::Held;
    let groups = if held {
        Vec::new()
    } else {
        crate::rate_limit::active_group_holds(rlk, custom_name)
            .into_iter()
            .map(|(group, info)| GroupHoldJson {
                group,
                recovery_at: info.recovery_at,
                message: info.message,
            })
            .collect()
    };
    let state = if held {
        "limited"
    } else if !groups.is_empty() {
        "partial"
    } else if avail.status == crate::route_availability::RouteStatus::Degraded {
        "degraded"
    } else {
        "ok"
    };
    let info = held
        .then(|| crate::rate_limit::get_rate_limit_info(rlk, custom_name))
        .flatten();
    QuotaJson {
        state: state.to_string(),
        recovery_at: info.as_ref().and_then(|value| value.recovery_at.clone()),
        message: info.as_ref().and_then(|value| value.message.clone()),
        source: quota.source,
        groups,
        used_percent: quota.used_percent,
        resets_at: quota.resets_at,
        window,
        stale: quota.stale,
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

/// Catalog default: the rated row marked "default", else the first rated row.
/// Served-only rows are never a catalog default.
pub(crate) fn catalog_default_model(kind: AgentKind) -> Option<String> {
    let models: Vec<_> = crate::model_catalog::models_for_agent(&kind)
        .into_iter()
        .filter(|model| model.origin == crate::model_catalog::ModelOrigin::Catalog)
        .collect();
    models
        .iter()
        .find(|model| model.description.to_ascii_lowercase().contains("default"))
        .or_else(|| models.first())
        .map(|model| model.model.to_string())
}

/// The agent CLI's own configured default, where aid can read it.
pub(crate) fn cli_configured_default(kind: AgentKind) -> Option<String> {
    match kind {
        AgentKind::Codex => crate::agent::codex::cli_config::configured_model(),
        AgentKind::Qwen => crate::model_catalog::get_qwen_selected_model(),
        _ => None,
    }
}

/// Default model and its source: sticky `aid agent config --model`, a custom
/// agent's forced model, the CLI's own config, then the catalog default.
pub(crate) fn resolve_default_model(
    name: &str,
    kind: AgentKind,
    custom_config: Option<&CustomAgentConfig>,
) -> (Option<String>, Option<&'static str>) {
    let tagged = |source: &'static str| move |model: String| (model, source);
    let resolved = crate::agent_config::get_default_model(name)
        .map(tagged("sticky"))
        .or_else(|| custom_config.and_then(|c| c.forced_model.clone()).map(tagged("forced")))
        .or_else(|| {
            if custom_config.is_some() {
                return None;
            }
            cli_configured_default(kind)
                .map(tagged("cli_config"))
                .or_else(|| catalog_default_model(kind).map(tagged("catalog")))
        });
    match resolved {
        Some((model, source)) => (Some(model), Some(source)),
        None => (None, None),
    }
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

#[cfg(test)]
#[path = "agent_json_helpers_tests.rs"]
mod quota_probe_tests;
