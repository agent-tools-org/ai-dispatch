// Builds machine-readable agent inventory, quota, model, and history output.
// Exports JSON list and single-agent printers plus testable value generation.
// Deps: agent registry, model catalog, rate limits, Store, serde_json.

use anyhow::Result;
use chrono::Local;

use crate::agent::custom::CustomAgentConfig;
use crate::types::{AgentKind, Task, TaskFilter};
use crate::store::Store;
use crate::cmd::agent_history::get_agent_histories;

#[cfg(test)]
#[path = "agent_json_tests.rs"]
mod tests;

use crate::cmd::agent_json_types::{
    AgentListJson, AgentJson, HistoryJson, ModelsJson,
    AvailableModelJson, LoadJson,
};
use crate::cmd::agent_json_helpers::{
    build_quota_json, builtin_profile, catalog_default_model, command_installed,
    get_agent_capabilities, metering_label, rate_limit_kind,
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
    let installed_agents = crate::agent::detect_agents();
    if let Some(kind) = builtin_profile(name) {
        let running_tasks = store.list_tasks(TaskFilter::Running).unwrap_or_default();
        let histories = get_agent_histories(store, &[kind.as_str()])?;
        let history = histories.get(kind.as_str()).cloned().flatten();
        let agent_json = build_agent_json(kind, None, &running_tasks, &installed_agents, history)?;
        println!("{}", serde_json::to_string_pretty(&agent_json)?);
        return Ok(());
    }
    let custom_agents = crate::agent::registry::list_custom_agents();
    if let Some(config) = custom_agents.iter().find(|c| c.id.eq_ignore_ascii_case(name)) {
        let running_tasks = store.list_tasks(TaskFilter::Running).unwrap_or_default();
        let histories = get_agent_histories(store, &[config.id.as_str()])?;
        let history = histories.get(&config.id).cloned().flatten();
        let agent_json = build_agent_json(
            AgentKind::Custom,
            Some(config),
            &running_tasks,
            &installed_agents,
            history,
        )?;
        println!("{}", serde_json::to_string_pretty(&agent_json)?);
        return Ok(());
    }
    anyhow::bail!("Unknown agent '{name}'")
}

pub(crate) fn get_agents_list(store: &Store) -> Result<AgentListJson> {
    let installed_agents = crate::agent::detect_agents();
    get_agents_list_with_installed(store, &installed_agents)
}

pub(crate) fn get_agents_list_with_installed(
    store: &Store,
    installed_agents: &[AgentKind],
) -> Result<AgentListJson> {
    let running_tasks = store.list_tasks(TaskFilter::Running).unwrap_or_default();
    let custom = crate::agent::registry::list_custom_agents();
    let history_names: Vec<&str> = AgentKind::ALL_BUILTIN
        .iter()
        .map(|kind| kind.as_str())
        .chain(custom.iter().map(|config| config.id.as_str()))
        .collect();
    let histories = get_agent_histories(store, &history_names)?;
    let mut agents = Vec::new();
    
    for kind in AgentKind::ALL_BUILTIN {
        let history = histories.get(kind.as_str()).cloned().flatten();
        let agent = build_agent_json(*kind, None, &running_tasks, &installed_agents, history)?;
        agents.push(agent);
    }
    
    for config in &custom {
        let history = histories.get(&config.id).cloned().flatten();
        let agent = build_agent_json(
            AgentKind::Custom,
            Some(config),
            &running_tasks,
            &installed_agents,
            history,
        )?;
        agents.push(agent);
    }
    
    Ok(AgentListJson {
        generated_at: Local::now().to_rfc3339(),
        agents,
    })
}

fn build_agent_json(
    kind: AgentKind,
    custom_config: Option<&CustomAgentConfig>,
    running_tasks: &[Task],
    installed_agents: &[AgentKind],
    history: Option<HistoryJson>,
) -> Result<AgentJson> {
    let name = match custom_config {
        Some(config) => config.id.clone(),
        None => kind.as_str().to_string(),
    };
    
    let is_custom = custom_config.is_some();
    
    let installed = if let Some(config) = custom_config {
        command_installed(&config.command)
    } else {
        installed_agents.contains(&kind)
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

    let quota = build_quota_json(
        &rate_limit_kind(kind, custom_config),
        custom_config.map(|c| c.id.as_str()),
    );
    
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
                    capability: m.capability,
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
