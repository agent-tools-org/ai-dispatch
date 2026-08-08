use anyhow::Result;
use chrono::Local;

use crate::agent::custom::CustomAgentConfig;
use crate::types::{AgentKind, Task, TaskFilter};
use crate::store::Store;

#[cfg(test)]
#[path = "agent_json_tests.rs"]
mod tests;

use crate::cmd::agent_json_types::{
    AgentListJson, AgentJson, QuotaJson, ModelsJson, AvailableModelJson, LoadJson
};
use crate::cmd::agent_json_helpers::{
    command_installed, get_agent_capabilities, get_agent_history
};

pub fn print_agents_json(store: &Store) -> Result<()> {
    let list = get_agents_list(store)?;
    println!("{}", serde_json::to_string_pretty(&list)?);
    Ok(())
}

pub(crate) fn agents_list_value(store: &Store) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(get_agents_list(store)?)?)
}

pub fn print_agent_json(store: &Store, name: &str) -> Result<()> {
    if let Some(kind) = builtin_profile(name) {
        let running_tasks = store.list_tasks(TaskFilter::Running).unwrap_or_default();
        let agent_json = build_agent_json(store, kind, None, &running_tasks)?;
        println!("{}", serde_json::to_string_pretty(&agent_json)?);
        return Ok(());
    }
    let custom_agents = crate::agent::registry::list_custom_agents();
    if let Some(config) = custom_agents.iter().find(|c| c.id.eq_ignore_ascii_case(name)) {
        let running_tasks = store.list_tasks(TaskFilter::Running).unwrap_or_default();
        let agent_json = build_agent_json(store, AgentKind::Custom, Some(config), &running_tasks)?;
        println!("{}", serde_json::to_string_pretty(&agent_json)?);
        return Ok(());
    }
    anyhow::bail!("Unknown agent '{name}'")
}

fn get_agents_list(store: &Store) -> Result<AgentListJson> {
    let running_tasks = store.list_tasks(TaskFilter::Running).unwrap_or_default();
    let mut agents = Vec::new();
    
    for kind in AgentKind::ALL_BUILTIN {
        agents.push(build_agent_json(store, *kind, None, &running_tasks)?);
    }
    
    let custom = crate::agent::registry::list_custom_agents();
    for config in &custom {
        agents.push(build_agent_json(store, AgentKind::Custom, Some(config), &running_tasks)?);
    }
    
    Ok(AgentListJson {
        generated_at: Local::now().to_rfc3339(),
        agents,
    })
}

fn build_agent_json(
    store: &Store,
    kind: AgentKind,
    custom_config: Option<&CustomAgentConfig>,
    running_tasks: &[Task],
) -> Result<AgentJson> {
    let name = match custom_config {
        Some(config) => config.id.clone(),
        None => kind.as_str().to_string(),
    };
    
    let is_custom = custom_config.is_some();
    
    let installed = if let Some(config) = custom_config {
        command_installed(&config.command)
    } else {
        crate::agent::detect_agents().contains(&kind)
    };
    
    let disabled = crate::agent_config::is_agent_disabled(&name);
    
    // `trust_tier` keeps the JSON field name for callers; the value is the
    // provider-derived egress label (local | private-network | third-party | unknown).
    let (description, trust_tier) = if let Some(config) = custom_config {
        (
            config.display_name.clone(),
            crate::agent::egress::resolve_agent_egress(&config.id)
                .label()
                .to_string(),
        )
    } else if let Some((_, desc, _, _, _)) = kind.profile() {
        (
            desc.to_string(),
            crate::types::egress_for_cli(kind).label().to_string(),
        )
    } else {
        ("".to_string(), crate::types::EgressTier::Unknown.label().to_string())
    };
    
    let supports_session_resume = if is_custom {
        false
    } else {
        kind.supports_session_resume()
    };
    let (provider, metering) = if let Some(config) = custom_config {
        crate::types::provider_for_custom(
            config.provider.as_deref(),
            config.metering.as_deref(),
        )
    } else {
        crate::types::provider_for_cli(kind)
    };

    let quota = {
        let custom_name = custom_config.map(|c| c.id.as_str());
        let state = if crate::rate_limit::is_rate_limited(&kind, custom_name) {
            "limited".to_string()
        } else {
            "ok".to_string()
        };
        let info = crate::rate_limit::get_rate_limit_info(&kind, custom_name);
        let recovery_at = info.as_ref().and_then(|i| i.recovery_at.clone());
        let message = info.as_ref().and_then(|i| i.message.clone());
        QuotaJson {
            state,
            recovery_at,
            message,
            source: "marker".to_string(),
        }
    };
    
    let capabilities = get_agent_capabilities(kind, custom_config);
    
    let models = {
        let default_model = crate::agent_config::get_default_model(&name)
            .or_else(|| custom_config.and_then(|c| c.forced_model.clone()))
            .or_else(|| {
                if is_custom {
                    None
                } else {
                    catalog_default_model(kind)
                }
            });
        let budget_model = if is_custom {
            None
        } else {
            crate::model_catalog::budget_model(&kind).map(|s| s.to_string())
        };
        let available = if is_custom {
            Vec::new()
        } else {
            let available_models = crate::cmd::config::merged_agent_models()?;
            available_models.into_iter()
                .filter(|m| m.agent == kind)
                .map(|m| AvailableModelJson {
                    model: m.model,
                    tier: m.tier,
                    input_per_m: m.input_per_m,
                    output_per_m: m.output_per_m,
                })
                .collect()
        };
        ModelsJson {
            default: default_model,
            budget: budget_model,
            available,
        }
    };
    
    let running = if is_custom {
        running_tasks.iter()
            .filter(|t| t.agent == AgentKind::Custom && t.custom_agent_name.as_deref() == Some(&name))
            .count() as u64
    } else {
        running_tasks.iter()
            .filter(|t| t.agent == kind)
            .count() as u64
    };
    let load = LoadJson { running };
    
    let history = get_agent_history(store, &name, is_custom)?;
    
    Ok(AgentJson {
        name,
        kind: if is_custom { "custom".to_string() } else { "builtin".to_string() },
        installed,
        disabled,
        trust_tier,
        description,
        supports_session_resume,
        provider: provider.as_str().to_string(),
        metering: metering_label(metering),
        quota,
        capabilities,
        models,
        history,
        load,
    })
}

fn builtin_profile(name: &str) -> Option<AgentKind> {
    AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(name))
}

fn catalog_default_model(kind: AgentKind) -> Option<String> {
    let models = crate::model_catalog::models_for_agent(&kind);
    models
        .iter()
        .find(|m| m.description.to_ascii_lowercase().contains("default"))
        .or_else(|| models.first())
        .map(|m| m.model.to_string())
}

/// Machine-readable metering shape, so the caller can route on it. An exhausted
/// `spend_budget` does not recover with time — only a top-up clears it — and a
/// `per_model_family` pool says nothing about that provider's other families.
/// Collapsing these into one "limited" flag is what stranded a working agy
/// claude allowance behind an exhausted gemini one.
fn metering_label(shape: crate::types::MeteringShape) -> String {
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
