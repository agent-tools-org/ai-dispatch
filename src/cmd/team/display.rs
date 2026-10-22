// Display helpers for the team command.
// Exports: command phases used by the parent module.
// Deps: parent command types and supporting configuration.

use crate::config::{team, toolbox};
use anyhow::{Result, bail};
pub(super) fn show_team(name: &str) -> Result<()> {
    let Some(config) = team::resolve_team(name) else {
        bail!("Team '{name}' not found. Use `aid team list` to see available teams.");
    };
    println!("Team: {}", config.id);
    println!("  Display name: {}", config.display_name);
    if !config.description.is_empty() {
        println!("  Description: {}", config.description);
    }
    if !config.preferred_agents.is_empty() {
        println!("  Preferred agents: {}", config.preferred_agents.join(", "));
    }
    if let Some(ref default) = config.default_agent {
        println!("  Default agent: {}", default);
    }
    print_team_knowledge(name);
    if !config.rules.is_empty() {
        println!("  Rules: {} rule(s)", config.rules.len());
        for rule in &config.rules {
            println!("    - {rule}");
        }
    }
    print_team_overrides(&config);
    print_team_toolbox(name, &config);
    Ok(())
}

fn print_team_knowledge(name: &str) {
    // Knowledge info
    let knowledge_dir = team::knowledge_dir(name);
    let knowledge_index = team::knowledge_index(name);
    if knowledge_index.is_file() {
        let entry_count = std::fs::read_to_string(&knowledge_index)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("- "))
            .count();
        println!(
            "  Knowledge: {} entries ({})",
            entry_count,
            knowledge_index.display()
        );
    } else {
        println!(
            "  Knowledge: (none — create {}/KNOWLEDGE.md)",
            knowledge_dir.parent().unwrap_or(&knowledge_dir).display()
        );
    }
}

fn print_team_overrides(config: &team::TeamConfig) {
    if !config.overrides.is_empty() {
        println!("  Overrides:");
        for (agent, overrides) in &config.overrides {
            let mut fields = Vec::new();
            if let Some(v) = overrides.research {
                fields.push(format!("research={v}"));
            }
            if let Some(v) = overrides.simple_edit {
                fields.push(format!("simple_edit={v}"));
            }
            if let Some(v) = overrides.complex_impl {
                fields.push(format!("complex_impl={v}"));
            }
            if let Some(v) = overrides.frontend {
                fields.push(format!("frontend={v}"));
            }
            if let Some(v) = overrides.debugging {
                fields.push(format!("debugging={v}"));
            }
            if let Some(v) = overrides.testing {
                fields.push(format!("testing={v}"));
            }
            if let Some(v) = overrides.refactoring {
                fields.push(format!("refactoring={v}"));
            }
            if let Some(v) = overrides.documentation {
                fields.push(format!("documentation={v}"));
            }
            println!("    {}: {}", agent, fields.join(", "));
        }
    }
}

fn print_team_toolbox(name: &str, config: &team::TeamConfig) {
    // Toolbox info
    let installed_tools = toolbox::list_team_tools(name);
    if !config.toolbox.tools.is_empty() || !installed_tools.is_empty() {
        let configured = config.toolbox.tools.len();
        let installed = installed_tools.len();
        println!("  Toolbox: {} configured, {} installed", configured, installed);
        if !config.toolbox.tools.is_empty() {
            println!("    Tools: {}", config.toolbox.tools.join(", "));
        }
        if !config.toolbox.auto_inject.is_empty() {
            println!("    Auto-inject: {}", config.toolbox.auto_inject.join(", "));
        }
        for tool in &installed_tools {
            println!(
                "    - {} ({}): {}",
                tool.name,
                tool.scope.label(),
                tool.description
            );
        }
    }
}
