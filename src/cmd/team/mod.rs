// Handler for `aid team` subcommands — manage team definitions.
// Exports: run_team_command.
// Deps: crate::config::team, crate::support::paths, std::fs.

mod display;
use display::show_team;

use anyhow::{Result, bail};
use std::fs;

use crate::cli::TeamAction;
use crate::config::team;
use crate::config::toolbox;
use crate::support::sanitize;

const TEAM_TEMPLATE: &str = r#"[team]
id = "{name}"
display_name = "{display_name}"
description = ""

# Soft preference for aid advise scoring (all agents remain available)
preferred_agents = []
# default_agent = "codex"

# Team toolbox: tools available to agents in this team context
# [team.toolbox]
# tools = ["lint-check", "test-runner"]
# auto_inject = ["lint-check"]

# Optional: override capability scores for agents within this team context
# [team.overrides.opencode]
# simple_edit = 10
# debugging = 6
"#;

pub fn run_team_command(action: TeamAction) -> Result<()> {
    match action {
        TeamAction::List => list_teams(),
        TeamAction::Show { name } => show_team(&name),
        TeamAction::Create { name } => create_team(&name),
        TeamAction::Delete { name } => delete_team(&name),
    }
}

fn list_teams() -> Result<()> {
    let teams = team::list_teams();
    if teams.is_empty() {
        println!("No teams configured.");
        println!("Use `aid team create <name>` to define a team.");
        return Ok(());
    }
    println!(
        "{:<12} {:<24} {:<10} {:<8} Default",
        "ID", "Name", "Preferred", "Knowledge"
    );
    println!("{}", "-".repeat(72));
    for t in &teams {
        let knowledge_index = team::knowledge_index(&t.id);
        let knowledge_count = if knowledge_index.is_file() {
            std::fs::read_to_string(&knowledge_index)
                .unwrap_or_default()
                .lines()
                .filter(|l| l.starts_with("- "))
                .count()
        } else {
            0
        };
        println!(
            "{:<12} {:<24} {:<10} {:<8} {}",
            t.id,
            t.display_name,
            t.preferred_agents.len(),
            knowledge_count,
            t.default_agent.as_deref().unwrap_or("-"),
        );
    }
    Ok(())
}

fn create_team(name: &str) -> Result<()> {
    if team::team_exists(name) {
        bail!("Team '{name}' already exists");
    }
    let dir = team::teams_dir();
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{name}.toml"));
    let display_name = title_case(name);
    let contents = TEAM_TEMPLATE
        .replace("{name}", name)
        .replace("{display_name}", &display_name);
    fs::write(&target, contents)?;

    // Scaffold knowledge directory and index
    let knowledge_dir = team::knowledge_dir(name);
    fs::create_dir_all(&knowledge_dir)?;
    let knowledge_index = team::knowledge_index(name);
    fs::write(
        &knowledge_index,
        format!(
            "# {display_name} — Team Knowledge\n\n<!-- Add knowledge entries as: - \
            [topic](knowledge/file.md) — description -->\n"
        ),
    )?;

    // Scaffold tools directory
    let tools_dir = toolbox::team_tools_dir(name);
    fs::create_dir_all(tools_dir.join("scripts"))?;

    println!("Created {}", target.display());
    println!("Knowledge: {}", knowledge_index.display());
    println!("Tools: {}", tools_dir.display());
    Ok(())
}

fn delete_team(name: &str) -> Result<()> {
    sanitize::validate_name(name, "team")?;
    let target = team::teams_dir().join(format!("{name}.toml"));
    if !target.is_file() {
        bail!("Team '{name}' does not exist");
    }
    fs::remove_file(&target)?;
    println!("Removed team '{name}'");
    Ok(())
}

fn title_case(name: &str) -> String {
    name.split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "team_tests.rs"]
mod tests;
