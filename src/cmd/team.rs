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

# Soft preference for auto-selection (all agents remain available)
preferred_agents = []
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

fn show_team(name: &str) -> Result<()> {
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
    // Knowledge info
    let knowledge_dir = team::knowledge_dir(name);
    let knowledge_index = team::knowledge_index(name);
    if knowledge_index.is_file() {
        let entry_count = std::fs::read_to_string(&knowledge_index)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("- "))
            .count();
        println!("  Knowledge: {} entries ({})", entry_count, knowledge_index.display());
    } else {
        println!("  Knowledge: (none — create {}/KNOWLEDGE.md)", knowledge_dir.parent().unwrap_or(&knowledge_dir).display());
    }
    if !config.rules.is_empty() {
        println!("  Rules: {} rule(s)", config.rules.len());
        for rule in &config.rules {
            println!("    - {rule}");
        }
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

    // Scaffold knowledge directory and index
    let knowledge_dir = team::knowledge_dir(name);
    fs::create_dir_all(&knowledge_dir)?;
    let knowledge_index = team::knowledge_index(name);
    fs::write(
        &knowledge_index,
        format!("# {display_name} — Team Knowledge\n\n<!-- Add knowledge entries as: - [topic](knowledge/file.md) — description -->\n"),
    )?;

    println!("Created {}", target.display());
    println!("Knowledge: {}", knowledge_index.display());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    fn test_env() -> (TempDir, AidHomeGuard) {
        let temp = TempDir::new().unwrap();
        let guard = AidHomeGuard::set(temp.path());
        (temp, guard)
    }

    fn team_file(name: &str) -> PathBuf {
        team::teams_dir().join(format!("{name}.toml"))
    }

    fn write_team_file(name: &str, contents: &str) {
        let dir = team::teams_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), contents).unwrap();
    }

    #[test]
    fn create_team_happy_path() {
        let (_temp, _guard) = test_env();
        run_team_command(TeamAction::Create {
            name: "ops".to_string(),
        })
        .unwrap();

        let file = team_file("ops");
        assert!(file.is_file());
        assert!(team::knowledge_dir("ops").is_dir());
        let knowledge_index = team::knowledge_index("ops");
        let contents = fs::read_to_string(&knowledge_index).unwrap();
        assert!(contents.contains("# Ops — Team Knowledge"));
    }

    #[test]
    fn create_team_duplicate_name_error() {
        let (_temp, _guard) = test_env();
        run_team_command(TeamAction::Create {
            name: "ops".to_string(),
        })
        .unwrap();
        let err = run_team_command(TeamAction::Create {
            name: "ops".to_string(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn delete_team_happy_path() {
        let (_temp, _guard) = test_env();
        run_team_command(TeamAction::Create {
            name: "removable".to_string(),
        })
        .unwrap();
        let file = team_file("removable");
        assert!(file.is_file());
        run_team_command(TeamAction::Delete {
            name: "removable".to_string(),
        })
        .unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn delete_team_missing_name_error() {
        let (_temp, _guard) = test_env();
        let err = run_team_command(TeamAction::Delete {
            name: "ghost".to_string(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn list_teams_no_teams() {
        let (_temp, _guard) = test_env();
        run_team_command(TeamAction::List).unwrap();
    }

    #[test]
    fn list_teams_multiple_entries() {
        let (_temp, _guard) = test_env();
        run_team_command(TeamAction::Create {
            name: "alpha".to_string(),
        })
        .unwrap();
        run_team_command(TeamAction::Create {
            name: "beta".to_string(),
        })
        .unwrap();
        run_team_command(TeamAction::List).unwrap();
    }

    #[test]
    fn show_team_with_overrides() {
        let (_temp, _guard) = test_env();
        let contents = r#"
            [team]
            id = "ops"
            display_name = "Operations"
            preferred_agents = ["codex"]
            default_agent = "opencode"
            description = "Ops work"

            [team.overrides.codex]
            simple_edit = 5
            debugging = 3
        "#;
        write_team_file("ops", contents);
        run_team_command(TeamAction::Show {
            name: "ops".to_string(),
        })
        .unwrap();
    }

    #[test]
    fn show_team_without_overrides() {
        let (_temp, _guard) = test_env();
        let contents = r#"
            [team]
            id = "solo"
            display_name = "Solo"
            preferred_agents = []
        "#;
        write_team_file("solo", contents);
        run_team_command(TeamAction::Show {
            name: "solo".to_string(),
        })
        .unwrap();
    }

    #[test]
    fn show_team_not_found_error() {
        let (_temp, _guard) = test_env();
        let err = run_team_command(TeamAction::Show {
            name: "missing".to_string(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn title_case_variants() {
        assert_eq!(title_case("my-team"), "My Team");
        assert_eq!(title_case("dev_ops"), "Dev Ops");
        assert_eq!(title_case("solo"), "Solo");
        assert_eq!(title_case(""), "");
    }
}
