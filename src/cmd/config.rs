// Handler for `aid config` subcommands.
// Exports: run(), merged_agent_models(), model_catalog re-exports
// Deps: model_catalog, config_display, agent registry, store

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::Arc;

use crate::agent;
use crate::agent::registry;
use crate::agent_config;
use crate::cli_actions::ConfigAction;
use crate::rate_limit;
use crate::skills;
use crate::store::Store;
use crate::templates;
use crate::types::{AgentKind, TaskFilter};

#[path = "config_display.rs"]
mod config_display;
#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

use config_display::{agent_profile, compute_agent_history, compute_model_history, format_capabilities};
use crate::model_catalog::AGENT_PROFILES;
pub(crate) use crate::model_catalog::{budget_model, merged_agent_models};
use crate::model_catalog::PricingResponse;

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

#[cfg(not(test))]
fn maybe_refresh_pricing_if_stale() {
    use std::time::{Duration, SystemTime};

    if std::env::var_os("AID_NO_PRICING_REFRESH").is_some() {
        return;
    }
    let path = crate::paths::pricing_path();
    const DAY_SECS: u64 = 24 * 60 * 60;
    let needs_refresh = match fs::metadata(&path) {
        Err(_) => true,
        Ok(meta) => match meta.modified() {
            Err(_) => true,
            Ok(mtime) => {
                SystemTime::now()
                    .duration_since(mtime)
                    .unwrap_or(Duration::ZERO)
                    > Duration::from_secs(DAY_SECS)
            }
        },
    };
    if !needs_refresh {
        return;
    }
    let path_str = path.to_string_lossy().into_owned();
    let url = "https://aid.agent-tools.org/api/pricing";
    let _child = Command::new("curl")
        .args(["-fsSL", "-o", &path_str, "-z", &path_str, url])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn print_agents(store: &Arc<Store>) {
    #[cfg(not(test))]
    maybe_refresh_pricing_if_stale();

    let installed = agent::detect_agents();
    let (history, model_history) = match store.list_tasks(TaskFilter::All) {
        Ok(tasks) => (compute_agent_history(&tasks), compute_model_history(&tasks)),
        Err(_) => (HashMap::new(), HashMap::new()),
    };
    for (kind, _, _, _, _) in AGENT_PROFILES {
        if !builtin_agent_visible(*kind) {
            continue;
        }
        let status = if installed.contains(kind) { "✓" } else { "✗" };
        let profile = agent_profile(*kind, installed.contains(kind), history.get(kind), &model_history);
        println!("{} {}\n{}", status, kind.as_str(), profile);
    }
    let custom_agents = registry::list_custom_agents();
    if custom_agents.is_empty() {
        println!("\nCustom agents: none found.");
    } else {
        println!("\nCustom agents:");
        for agent in custom_agents {
            if agent_config::is_agent_disabled(&agent.id) {
                continue;
            }
            let install_status = if command_installed(&agent.command) {
                "installed"
            } else {
                "not installed"
            };
            println!("  - Name: {}", agent.id);
            println!("    Display name: {}", agent.display_name);
            println!("    Command: {} ({})", agent.command, install_status);
            println!("    Capabilities: {}", format_capabilities(&agent.capabilities));
            // Rate-limit status: uses the custom agent's own id in the hint
            // (clear-limit <id>) rather than the kind constant "custom".
            if rate_limit::is_rate_limited(&AgentKind::Custom, Some(agent.id.as_str())) {
                let hint = match rate_limit::dispatch_blocking_hold(
                    &AgentKind::Custom,
                    Some(agent.id.as_str()),
                ) {
                    Some(h) => format!(" — {h}"),
                    None => String::new(),
                };
                println!("    Status: LIMITED{hint}");
            }
        }
    }
    if let Some(line) = disabled_summary_line(&disabled_agent_names()) {
        println!("\n{line}");
    }
}

fn builtin_agent_visible(kind: AgentKind) -> bool { !agent_config::is_agent_disabled(kind.as_str()) }

fn disabled_agent_names() -> Vec<String> {
    let mut names: Vec<String> = agent_config::load_agent_config()
        .into_iter()
        .filter_map(|(name, defaults)| defaults.disabled.then_some(name))
        .collect();
    names.sort();
    names
}

fn disabled_summary_line(names: &[String]) -> Option<String> {
    if names.is_empty() { return None; }
    let hint = if names.len() == 1 { names[0].as_str() } else { "<name>" };
    Some(format!("Disabled: {} (enable with aid agent config {hint} --enable)", names.join(", ")))
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
            let input = am.input_per_m.map(|value| format!("${value:.2}")).unwrap_or_else(|| "unknown".to_string());
            let output = am.output_per_m.map(|value| format!("${value:.2}")).unwrap_or_else(|| "unknown".to_string());
            println!(
                "{:<10} {:<25} {:>10} {:>10} {:>10} {}",
                agent.as_str(),
                am.model,
                am.tier,
                input,
                output,
                am.description
            );
        }
    }
    Ok(())
}

fn clear_limit(agent: &str) -> Result<()> {
    if agent == "all" {
        for (kind, _, _, _, _) in AGENT_PROFILES {
            if rate_limit::clear_all_rate_limits_for_agent(kind, None) {
                println!("Cleared rate-limit for {}", kind.as_str());
            }
        }
        for config in crate::agent::registry::list_custom_agents() {
            if rate_limit::clear_all_rate_limits_for_agent(
                &AgentKind::Custom,
                Some(config.id.as_str()),
            ) {
                println!("Cleared rate-limit for {}", config.id);
            }
        }
        return Ok(());
    }
    if let Some(kind) = AgentKind::parse_str(agent) {
        if rate_limit::clear_all_rate_limits_for_agent(&kind, None) {
            println!("Cleared rate-limit for {}", kind.as_str());
        } else {
            println!("{} is not rate-limited", agent);
        }
        return Ok(());
    }
    if crate::agent::registry::custom_agent_exists(agent) {
        if rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Custom, Some(agent)) {
            println!("Cleared rate-limit for {agent}");
        } else {
            println!("{agent} is not rate-limited");
        }
        return Ok(());
    }
    anyhow::bail!("Unknown agent: {agent}");
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
