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
#[cfg(test)]
#[path = "agent_json_models_tests.rs"]
mod models_tests;
#[cfg(test)]
#[path = "agent_json_evidence_tests.rs"]
mod evidence_tests;

use crate::cmd::agent_json_types::{
    AgentListJson, AgentJson, HistoryJson, ModelsJson,
    AvailableModelJson, LoadJson,
};
use crate::cmd::agent_json_helpers::{
    build_quota_json, builtin_profile, command_installed,
    agent_metadata, rate_limit_kind,
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
    let builtins: Vec<AgentKind> = crate::agent::routable_builtins().collect();
    let history_names: Vec<&str> = builtins
        .iter()
        .map(|kind| kind.as_str())
        .chain(custom.iter().map(|config| config.id.as_str()))
        .collect();
    let histories = get_agent_histories(store, &history_names)?;
    let mut agents = Vec::new();
    
    for kind in &builtins {
        let history = histories.get(kind.as_str()).cloned().flatten();
        let agent = build_agent_json(*kind, None, &running_tasks, installed_agents, history)?;
        agents.push(agent);
    }
    
    for config in &custom {
        let history = histories.get(&config.id).cloned().flatten();
        let agent = build_agent_json(
            AgentKind::Custom,
            Some(config),
            &running_tasks,
            installed_agents,
            history,
        )?;
        agents.push(agent);
    }
    
    Ok(AgentListJson {
        generated_at: Local::now().to_rfc3339(),
        agents,
        sources: crate::scores::sources(),
    })
}

/// What `aid run <agent>` launches with no flags or declared profile.
fn default_run_model(
    name: &str, kind: AgentKind, custom: Option<&crate::agent::custom::CustomAgentConfig>,
) -> crate::agent::run_model::RunModel {
    let selection = crate::config::load_config().map(|config| config.selection).unwrap_or_default();
    crate::agent::run_model::resolve_run_model(&crate::agent::run_model::RunModelInput {
        agent_name: name, kind, explicit_model: None, force_default: false, custom,
        budget_mode: selection.budget_mode, declared_budget: None, declared_difficulty: None,
        smart_routing: selection.smart_routing,
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
    
    let (description, trust_tier, provider, metering) = agent_metadata(kind, custom_config);
    let supports_session_resume = !is_custom && kind.supports_session_resume();
    let quota = build_quota_json(
        &rate_limit_kind(kind, custom_config),
        custom_config.map(|c| c.id.as_str()),
    );
    
    let auth = crate::auth_marker::auth_status(kind, custom_config.map(|c| c.id.as_str()));

    let models = build_models_json(&name, kind, custom_config)?;
    Ok(AgentJson {
        name,
        kind: if is_custom { "custom".to_string() } else { "builtin".to_string() },
        installed,
        disabled,
        trust_tier,
        description,
        supports_session_resume,
        provider,
        metering,
        quota,
        auth,
        models,
        history,
        load: agent_load(kind, custom_config, running_tasks),
    })
}

fn build_models_json(
    name: &str, kind: AgentKind, custom_config: Option<&CustomAgentConfig>,
) -> Result<ModelsJson> {
    let is_custom = custom_config.is_some();
    let run_model = default_run_model(name, kind, custom_config);
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
            .map(|m| {
                let price = crate::cost::resolve_pricing(Some(&m.model), kind);
                let evidence = crate::scores::evidence(kind, Some(&m.model),
                    crate::agent::classifier::TaskCategory::ComplexImpl);
                AvailableModelJson {
                    model: m.model,
                    tier: m.tier,
                    input_per_m: price.map(|p| p.input_per_m),
                    output_per_m: price.map(|p| p.output_per_m),
                    rated: evidence.capability.is_some(),
                    capability: evidence.capability,
                    capability_evidence: evidence,
                    source: m.origin.label().to_string(),
                }
            })
            .collect()
    };
    Ok(ModelsJson {
        default_source: run_model.model.as_ref().map(|_| run_model.source.as_str().to_string()),
        default: run_model.model,
        budget: budget_model,
        available,
    })
}

fn agent_load(
    kind: AgentKind, custom_config: Option<&CustomAgentConfig>, running_tasks: &[Task],
) -> LoadJson {
    let running = if let Some(config) = custom_config {
        running_tasks.iter()
            .filter(|t| t.agent == AgentKind::Custom
                && t.custom_agent_name.as_deref() == Some(config.id.as_str()))
            .count() as u64
    } else {
        running_tasks.iter()
            .filter(|t| t.agent == kind)
            .count() as u64
    };
    LoadJson { running }
}
