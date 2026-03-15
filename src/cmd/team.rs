// Handler for `aid team` subcommands — manage team definitions.
// Exports: TeamAction, run_team_command.
// Deps: crate::team, crate::paths, std::fs.

use anyhow::{bail, Result};
use std::fs;

use crate::team;

const TEAM_TEMPLATE: &str = r#"[team]
id = "{name}"
display_name = "{display_name}"
description = ""
agents = []
# default_agent = "codex"

# Optional: override capability scores for agents within this team context
# [team.overrides.opencode]
# simple_edit = 10
# debugging = 6
"#;

pub enum TeamAction {
    List,
    Show { name: String },
    Create { name: String },
    Delete { name: String },
}

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
        "{:<12} {:<24} {:<8} {}",
        "ID", "Name", "Agents", "Default"
    );
    println!("{}", "-".repeat(60));
    for t in &teams {
        println!(
            "{:<12} {:<24} {:<8} {}",
            t.id,
            t.display_name,
            t.agents.len(),
            t.default_agent.as_deref().unwrap_or("-"),
        );
    }
    Ok(())
}

fn show_team(name: &str) -> Result<()> {
    let Some(config) = team::resolve_team(name) else {
        bail!("Team '{name}' not found. Use `aid team list` to see available teams.");
    };
    println!("Team: {}", config.id);
    println!("  Display name: {}", config.display_name);
    if !config.description.is_empty() {
        println!("  Description: {}", config.description);
    }
    println!("  Agents: {}", config.agents.join(", "));
    if let Some(ref default) = config.default_agent {
        println!("  Default agent: {}", default);
    }
    if !config.overrides.is_empty() {
        println!("  Overrides:");
        for (agent, overrides) in &config.overrides {
            let mut fields = Vec::new();
            if let Some(v) = overrides.research { fields.push(format!("research={v}")); }
            if let Some(v) = overrides.simple_edit { fields.push(format!("simple_edit={v}")); }
            if let Some(v) = overrides.complex_impl { fields.push(format!("complex_impl={v}")); }
            if let Some(v) = overrides.frontend { fields.push(format!("frontend={v}")); }
            if let Some(v) = overrides.debugging { fields.push(format!("debugging={v}")); }
            if let Some(v) = overrides.testing { fields.push(format!("testing={v}")); }
            if let Some(v) = overrides.refactoring { fields.push(format!("refactoring={v}")); }
            if let Some(v) = overrides.documentation { fields.push(format!("documentation={v}")); }
            println!("    {}: {}", agent, fields.join(", "));
        }
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
    println!("Created {}", target.display());
    println!("Edit the file to add agents to the team.");
    Ok(())
}

fn delete_team(name: &str) -> Result<()> {
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
