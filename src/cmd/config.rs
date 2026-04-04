// Handler for `aid config` subcommands.
// Exports: run(), load_pricing_overrides(), merged_agent_models()
// Deps: config_models, config_display, agent registry, store

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::Arc;

use crate::agent;
use crate::agent::registry;
use crate::cli_actions::ConfigAction;
use crate::rate_limit;
use crate::skills;
use crate::store::Store;
use crate::templates;
use crate::types::{AgentKind, TaskFilter};

#[path = "config_display.rs"]
mod config_display;
#[path = "config_models.rs"]
mod config_models;
#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

pub use config_display::{budget_model, models_for_agent};
use config_display::{agent_profile, compute_agent_history, compute_model_history, format_capabilities};
pub use config_models::{AGENT_MODELS, AGENT_PROFILES, PricingFileModel, ResolvedAgentModel};

#[derive(Debug, Clone, Deserialize)]
struct PricingResponse {
    models: Vec<PricingFileModel>,
}

pub fn run(store: &Arc<Store>, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Agents => print_agents(store),
        ConfigAction::Skills => print_skills()?,
        ConfigAction::PromptBudget => print_prompt_budget()?,
        ConfigAction::Templates => print_templates(),
        ConfigAction::Pricing { update } => print_pricing(update)?,
        ConfigAction::ClearLimit { agent } => clear_limit(&agent)?,
        ConfigAction::AddAgent { .. } => {
            println!("Custom agent registration not yet implemented");
        }
    }
    Ok(())
}

fn print_agents(store: &Arc<Store>) {
    let installed = agent::detect_agents();
    let (history, model_history) = match store.list_tasks(TaskFilter::All) {
        Ok(tasks) => (compute_agent_history(&tasks), compute_model_history(&tasks)),
        Err(_) => (HashMap::new(), HashMap::new()),
    };
    for (kind, _, _, _, _) in AGENT_PROFILES {
        let status = if installed.contains(kind) { "✓" } else { "✗" };
        let profile = agent_profile(*kind, installed.contains(kind), history.get(kind), &model_history);
        println!("{} {}\n{}", status, kind.as_str(), profile);
    }
    let custom_agents = registry::list_custom_agents();
    if custom_agents.is_empty() {
        println!("\nCustom agents: none found.");
        return;
    }
    println!("\nCustom agents:");
    for agent in custom_agents {
        let install_status = if command_installed(&agent.command) {
            "installed"
        } else {
            "not installed"
        };
        println!("  - Name: {}", agent.id);
        println!("    Display name: {}", agent.display_name);
        println!("    Command: {} ({})", agent.command, install_status);
        println!("    Capabilities: {}", format_capabilities(&agent.capabilities));
    }
}

fn print_skills() -> Result<()> {
    let skills = skills::list_skills()?;
    if skills.is_empty() {
        println!("No skills found in ~/.aid/skills/.");
        println!("  Run `aid init` to install default skills.");
        return Ok(());
    }
    println!("Available skills:");
    for skill in &skills {
        println!("  - {skill}");
    }
    Ok(())
}

fn print_prompt_budget() -> Result<()> {
    let skills = skills::list_skills()?;
    if skills.is_empty() {
        println!("No skills found in ~/.aid/skills/.");
        println!("  Run `aid init` to install default skills.");
        return Ok(());
    }
    println!("Skill Token Budget:");
    let mut total_tokens = 0usize;
    for skill in &skills {
        let (_, tokens) = skills::measure_skill_tokens(skill)?;
        total_tokens += tokens;
        println!("  {:14} ~{} tokens", skill, tokens);
    }
    println!("  ─────────────────────");
    println!("  Total:         ~{} tokens", total_tokens);
    Ok(())
}

fn print_templates() {
    let templates = templates::list_templates();
    if templates.is_empty() {
        println!("No templates found in ~/.aid/templates/.");
        println!("  Run `aid init` to install default templates.");
        return;
    }
    println!("Available templates:");
    for template in &templates {
        println!("  - {template}");
    }
}

fn print_pricing(update: bool) -> Result<()> {
    if update {
        let updated = update_pricing_file()?;
        println!("Updated {updated} models in {}.", crate::paths::pricing_path().display());
    }
    let pricing = merged_agent_models()?;
    println!(
        "{:<10} {:<25} {:>10} {:>10} {:>10} Description",
        "Agent", "Model", "Tier", "Input/M", "Output/M"
    );
    println!("{}", "-".repeat(85));
    for &agent in AgentKind::ALL_BUILTIN {
        for am in pricing.iter().filter(|model| model.agent == agent) {
            println!(
                "{:<10} {:<25} {:>10} ${:>9.2} ${:>9.2} {}",
                agent.as_str(),
                am.model,
                am.tier,
                am.input_per_m,
                am.output_per_m,
                am.description
            );
        }
    }
    Ok(())
}

fn clear_limit(agent: &str) -> Result<()> {
    if agent == "all" {
        for (kind, _, _, _, _) in AGENT_PROFILES {
            if rate_limit::clear_rate_limit(kind) {
                println!("Cleared rate-limit for {}", kind.as_str());
            }
        }
        return Ok(());
    }
    let Some(kind) = AgentKind::parse_str(agent) else {
        anyhow::bail!("Unknown agent: {agent}");
    };
    if rate_limit::clear_rate_limit(&kind) {
        println!("Cleared rate-limit for {}", kind.as_str());
    } else {
        println!("{} is not rate-limited", agent);
    }
    Ok(())
}

pub fn load_pricing_overrides() -> Result<Vec<PricingFileModel>> {
    let path = crate::paths::pricing_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    let response: PricingResponse = serde_json::from_str(&contents)?;
    Ok(response.models)
}

pub fn merged_agent_models() -> Result<Vec<ResolvedAgentModel>> {
    let mut merged = Vec::with_capacity(AGENT_MODELS.len());
    let mut indexes = HashMap::new();
    for model in AGENT_MODELS {
        indexes.insert((model.agent, model.model.to_lowercase()), merged.len());
        merged.push(ResolvedAgentModel::from(model));
    }
    for model in load_pricing_overrides()? {
        let Some(agent) = AgentKind::parse_str(&model.agent) else {
            continue;
        };
        let key = (agent, model.model.to_lowercase());
        if let Some(index) = indexes.get(&key).copied() {
            merged[index].apply_override(model);
            continue;
        }
        indexes.insert(key, merged.len());
        merged.push(ResolvedAgentModel::from_override(agent, model));
    }
    Ok(merged)
}

fn update_pricing_file() -> Result<usize> {
    let output = Command::new("curl")
        .args(["-fsSL", "https://aid.agent-tools.org/api/pricing"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("curl failed with status {}", output.status);
    }
    let body = String::from_utf8(output.stdout)?;
    let response: PricingResponse = serde_json::from_str(&body)?;
    let path = crate::paths::pricing_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body)?;
    Ok(response.models.len())
}

fn command_installed(command: &str) -> bool {
    let binary = command.split_whitespace().next().unwrap_or_default();
    if binary.is_empty() {
        return false;
    }
    Command::new("which")
        .arg(binary)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
