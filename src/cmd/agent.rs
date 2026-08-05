// Handler for `aid agent` subcommands — manage custom agent definitions.
// Exports: AgentAction, run_agent_command.
// Deps: crate::agent::registry, crate::agent::custom, crate::paths, std::fs.
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::agent::custom::parse_config;
use crate::agent::registry;
use crate::agent_config;
use crate::paths;
use crate::sanitize;
use crate::types::AgentKind;

use crate::store::Store;

#[path = "agent_template.rs"]
mod agent_template;
use agent_template::{build_builtin_agent_toml, custom_agent_template, title_case};

use crate::cmd::agent_display::{print_custom_summary, show_builtin_profile, show_quota, list_agents};
use crate::cmd::agent_json;

pub enum AgentAction {
    List {
        json: bool,
    },
    Show {
        name: String,
        json: bool,
    },
    Config {
        name: String,
        model: Option<String>,
        idle_timeout: Option<u64>,
        disable: bool,
        enable: bool,
    },
    Add {
        name: String,
    },
    Remove {
        name: String,
    },
    Fork {
        name: String,
        new_name: Option<String>,
    },
    Quota,
}

pub fn run_agent_command(store: &Store, action: AgentAction) -> Result<()> {
    match action {
        AgentAction::List { json } => {
            if json {
                agent_json::print_agents_json(store)
            } else {
                list_agents()
            }
        }
        AgentAction::Show { name, json } => {
            if json {
                agent_json::print_agent_json(store, &name)
            } else {
                show_agent(&name)
            }
        }
        AgentAction::Config { name, model, idle_timeout, disable, enable } => {
            config_agent(&name, model.as_deref(), idle_timeout, disable, enable)
        }
        AgentAction::Add { name } => add_agent(&name),
        AgentAction::Remove { name } => remove_agent(&name),
        AgentAction::Fork { name, new_name } => fork_agent(&name, new_name.as_deref()),
        AgentAction::Quota => show_quota(),
    }
}

fn config_agent(
    name: &str,
    model: Option<&str>,
    idle_timeout: Option<u64>,
    disable: bool,
    enable: bool,
) -> Result<()> {
    if !is_builtin(name) && !registry::custom_agent_exists(name) {
        bail!("Unknown agent '{name}'");
    }
    if disable && enable {
        bail!("Cannot use --disable and --enable together");
    }
    let config_name = builtin_profile(name)
        .map(|kind| kind.as_str().to_string())
        .unwrap_or_else(|| name.to_string());
    if let Some(model) = model {
        agent_config::save_agent_default_model(&config_name, Some(model))?;
        println!("[aid] {config_name}: default model set to {model}");
    }
    if let Some(secs) = idle_timeout {
        let value = if secs == 0 { None } else { Some(secs) };
        agent_config::save_agent_idle_timeout(&config_name, value)?;
        if secs == 0 {
            println!("[aid] {config_name}: idle timeout cleared");
        } else {
            println!("[aid] {config_name}: idle timeout set to {secs}s");
        }
    }
    if disable {
        agent_config::save_agent_disabled(&config_name, true)?;
        println!("[aid] {config_name}: disabled");
    }
    if enable {
        agent_config::save_agent_disabled(&config_name, false)?;
        println!("[aid] {config_name}: enabled");
    }
    if model.is_none() && idle_timeout.is_none() && !disable && !enable {
        // No flags: show current config
        let current_model = agent_config::get_default_model(&config_name);
        let current_idle = agent_config::get_default_idle_timeout(&config_name);
        let disabled = agent_config::is_agent_disabled(&config_name);
        println!("[aid] {config_name} config:");
        println!("  model: {}", current_model.as_deref().unwrap_or("(default)"));
        println!("  idle_timeout: {}", current_idle.map_or("(default 600s)".to_string(), |s| format!("{s}s")));
        println!("  disabled: {}", disabled);
    }
    Ok(())
}



fn show_agent(name: &str) -> Result<()> {
    if let Some(kind) = builtin_profile(name) {
        show_builtin_profile(kind);
        return Ok(());
    }
    show_custom_agent(name)
}

fn add_agent(name: &str) -> Result<()> {
    if is_builtin(name) {
        bail!("Cannot add custom agent '{name}' because it conflicts with a built-in agent");
    }
    let dir = agent_dir();
    fs::create_dir_all(&dir)?;
    let target = custom_agent_path(name);
    if target.exists() {
        bail!("Agent '{name}' already exists at {}", target.display());
    }
    let display_name = title_case(name);
    let contents = custom_agent_template(name, &display_name);
    fs::write(&target, contents)?;
    println!("Created {}", target.display());
    println!("Edit the file to configure the agent.");
    Ok(())
}

fn remove_agent(name: &str) -> Result<()> {
    if is_builtin(name) {
        bail!("Cannot remove built-in agent '{name}'");
    }
    sanitize::validate_name(name, "agent")?;
    let target = custom_agent_path(name);
    if !target.is_file() {
        bail!("Custom agent '{name}' does not exist");
    }
    fs::remove_file(&target)?;
    println!("Removed {}", target.display());
    Ok(())
}

fn fork_agent(name: &str, new_name: Option<&str>) -> Result<()> {
    let target_name = new_name
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("{name}-custom"));
    if is_builtin(&target_name) {
        bail!("Cannot fork into '{target_name}' because it conflicts with a built-in agent");
    }

    let dir = agent_dir();
    fs::create_dir_all(&dir)?;
    let target = custom_agent_path(&target_name);
    if target.exists() {
        bail!(
            "Agent '{target_name}' already exists at {}",
            target.display()
        );
    }

    if let Some(kind) = builtin_profile(name) {
        let contents = build_builtin_agent_toml(&target_name, kind);
        fs::write(&target, contents)
            .with_context(|| format!("Failed to write {}", target.display()))?;
        println!("Created {}", target.display());
        println!("Edit the file to configure the agent.");
        return Ok(());
    }

    let source = custom_agent_path(name);
    if !source.is_file() {
        bail!(
            "Custom agent '{name}' not found (expected at {})",
            source.display()
        );
    }
    fs::copy(&source, &target).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source.display(),
            target.display()
        )
    })?;
    println!("Created {}", target.display());
    println!("Edit the file to configure the agent.");
    Ok(())
}

fn show_custom_agent(name: &str) -> Result<()> {
    let target = custom_agent_path(name);
    if !target.is_file() {
        bail!(
            "Custom agent '{name}' not found (expected at {})",
            target.display()
        );
    }
    let contents = fs::read_to_string(&target)
        .with_context(|| format!("Failed to read {}", target.display()))?;
    let config =
        parse_config(&contents).with_context(|| format!("Failed to parse {}", target.display()))?;
    print_custom_summary(&config, &target);
    println!("\nTOML preview:\n{}", contents);
    Ok(())
}

fn builtin_profile(name: &str) -> Option<AgentKind> {
    AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(name))
}

fn is_builtin(name: &str) -> bool {
    builtin_profile(name).is_some()
}

fn agent_dir() -> PathBuf {
    paths::aid_dir().join("agents")
}

fn custom_agent_path(name: &str) -> PathBuf {
    agent_dir().join(format!("{name}.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    #[test]
    fn show_quota_runs_without_panic() {
        let temp_dir = std::env::temp_dir().join("aid-quota-test-no-markers");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok(); // No rate-limit markers — should not panic
        let result = show_quota();
        assert!(result.is_ok());
    }
}
