// Handler for `aid tool` subcommands — manage team toolbox definitions.
// Exports: run_tool_command.
// Deps: crate::toolbox, crate::sanitize, anyhow, std::fs.

use anyhow::{bail, Context, Result};
use std::fs;

use crate::cli_actions::ToolAction;
use crate::sanitize;
use crate::toolbox;

const TOOL_TEMPLATE: &str = r#"[tool]
id = "{name}"
display_name = "{display_name}"
description = ""
command = "{name}"
# args = "<files...>"
# output_format = "text"
# tags = []
"#;

pub fn run_tool_command(action: ToolAction) -> Result<()> {
    match action {
        ToolAction::List { team } => list_tools(team.as_deref()),
        ToolAction::Show { name } => show_tool(&name),
        ToolAction::Add { name, team } => add_tool(&name, team.as_deref()),
        ToolAction::Remove { name } => remove_tool(&name),
        ToolAction::Test { name, args } => test_tool(&name, &args),
    }
}

fn list_tools(team: Option<&str>) -> Result<()> {
    let tools = if let Some(team_id) = team {
        toolbox::list_team_tools(team_id)
    } else {
        toolbox::resolve_toolbox(None, None)
    };
    if tools.is_empty() {
        println!("No tools configured.");
        println!("Use `aid tool add <name>` to create a tool definition.");
        return Ok(());
    }
    println!(
        "{:<16} {:<32} {:<8} Tags",
        "ID", "Description", "Scope"
    );
    println!("{}", "-".repeat(72));
    for tool in &tools {
        let tags = if tool.tags.is_empty() {
            "-".to_string()
        } else {
            tool.tags.join(", ")
        };
        let desc = truncate_str(&tool.description, 30);
        println!(
            "{:<16} {:<32} {:<8} {}",
            tool.name, desc, tool.scope.label(), tags
        );
    }
    Ok(())
}

fn show_tool(name: &str) -> Result<()> {
    let tool = toolbox::find_tool(name, None, None)?;
    println!("Tool: {}", tool.name);
    println!("  Display name: {}", tool.display_name);
    if !tool.description.is_empty() {
        println!("  Description: {}", tool.description);
    }
    println!("  Command: {}", tool.command);
    if !tool.args.is_empty() {
        println!("  Args: {}", tool.args);
    }
    println!("  Output: {}", tool.output_format);
    println!("  Scope: {}", tool.scope.label());
    if !tool.tags.is_empty() {
        println!("  Tags: {}", tool.tags.join(", "));
    }
    Ok(())
}

fn add_tool(name: &str, team: Option<&str>) -> Result<()> {
    sanitize::validate_name(name, "tool")?;
    let dir = match team {
        Some(id) => toolbox::team_tools_dir(id),
        None => toolbox::tools_dir(),
    };
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{name}.toml"));
    if target.is_file() {
        bail!("Tool '{name}' already exists at {}", target.display());
    }
    let display_name = title_case(name);
    let contents = TOOL_TEMPLATE
        .replace("{name}", name)
        .replace("{display_name}", &display_name);
    fs::write(&target, contents)?;
    println!("Created {}", target.display());
    Ok(())
}

fn remove_tool(name: &str) -> Result<()> {
    sanitize::validate_name(name, "tool")?;
    let target = toolbox::tools_dir().join(format!("{name}.toml"));
    if !target.is_file() {
        bail!("Tool '{name}' not found at {}", target.display());
    }
    fs::remove_file(&target)?;
    println!("Removed tool '{name}'");
    Ok(())
}

fn test_tool(name: &str, args: &[String]) -> Result<()> {
    let tool = toolbox::find_tool(name, None, None)?;
    println!("Running: {} {}", tool.command, args.join(" "));
    let status = std::process::Command::new(&tool.command)
        .args(args)
        .status()
        .with_context(|| format!("Failed to run tool command '{}'", tool.command))?;
    let code = status.code().unwrap_or(-1);
    if status.success() {
        println!("Tool '{}' completed successfully", name);
    } else {
        println!("Tool '{}' exited with status {}", name, code);
    }
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

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_actions::ToolAction;
    use crate::paths::AidHomeGuard;
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    fn test_env() -> (TempDir, AidHomeGuard) {
        let temp = TempDir::new().unwrap();
        let guard = AidHomeGuard::set(temp.path());
        (temp, guard)
    }

    fn tool_file(name: &str) -> PathBuf {
        toolbox::tools_dir().join(format!("{name}.toml"))
    }

    #[test]
    fn add_tool_creates_toml() {
        let (_temp, _guard) = test_env();
        run_tool_command(ToolAction::Add {
            name: "lint".to_string(),
            team: None,
        })
        .unwrap();
        assert!(tool_file("lint").is_file());
    }

    #[test]
    fn add_tool_duplicate_errors() {
        let (_temp, _guard) = test_env();
        run_tool_command(ToolAction::Add {
            name: "lint".to_string(),
            team: None,
        })
        .unwrap();
        let err = run_tool_command(ToolAction::Add {
            name: "lint".to_string(),
            team: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn remove_tool_deletes_file() {
        let (_temp, _guard) = test_env();
        run_tool_command(ToolAction::Add {
            name: "temp".to_string(),
            team: None,
        })
        .unwrap();
        assert!(tool_file("temp").is_file());
        run_tool_command(ToolAction::Remove {
            name: "temp".to_string(),
        })
        .unwrap();
        assert!(!tool_file("temp").exists());
    }

    #[test]
    fn remove_missing_tool_errors() {
        let (_temp, _guard) = test_env();
        let err = run_tool_command(ToolAction::Remove {
            name: "ghost".to_string(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn list_tools_no_tools() {
        let (_temp, _guard) = test_env();
        run_tool_command(ToolAction::List { team: None }).unwrap();
    }

    #[test]
    fn add_team_tool_creates_in_team_dir() {
        let (_temp, _guard) = test_env();
        run_tool_command(ToolAction::Add {
            name: "scanner".to_string(),
            team: Some("dev".to_string()),
        })
        .unwrap();
        let path = toolbox::team_tools_dir("dev").join("scanner.toml");
        assert!(path.is_file());
    }

    #[test]
    fn title_case_variants() {
        assert_eq!(title_case("lint-check"), "Lint Check");
        assert_eq!(title_case("my_tool"), "My Tool");
        assert_eq!(title_case("solo"), "Solo");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn show_tool_not_found_error() {
        let (_temp, _guard) = test_env();
        let err = run_tool_command(ToolAction::Show {
            name: "missing".to_string(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
