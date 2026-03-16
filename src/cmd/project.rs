// Project command handlers for the `aid project` CLI group.
// Exports: ProjectAction, run_project_command.
// Deps: crate::project, serde_json, std::{fs, io, path, process}.
use crate::project;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
pub enum ProjectAction {
    Init,
    Show,
}
pub fn run_project_command(action: ProjectAction) -> Result<()> {
    match action {
        ProjectAction::Init => init(),
        ProjectAction::Show => show(),
    }
}
fn init() -> Result<()> {
    let git_root = current_git_root()?;
    let project_id = prompt_project_id(&git_root)?;
    let profile = prompt_profile("Profile (hobby/standard/production)", "standard")?;
    let language = prompt_language(detect_language(&git_root).as_deref())?;
    let (project_path, knowledge_index) = write_project_config(&git_root, &project_id, &profile, language.as_deref())?;
    let config = project::load_project(&project_path)?;
    println!("Project: {}", config.id);
    println!("  Profile: {}", config.profile.as_deref().unwrap_or("-"));
    println!("  Language: {}", config.language.as_deref().unwrap_or("-"));
    println!("  File: {}", project_path.display());
    println!("  Knowledge: {}", knowledge_index.display());
    Ok(())
}
fn show() -> Result<()> {
    let config = project::detect_project().ok_or_else(|| {
        anyhow!("No project configuration found. Run `aid project init` in a git repository.")
    })?;
    let git_root = current_git_root()?;
    println!("Project: {}", config.id);
    println!("  Profile: {}", config.profile.as_deref().unwrap_or("-"));
    println!("  Team: {}", config.team.as_deref().unwrap_or("-"));
    println!("  Verify: {}", config.verify.as_deref().unwrap_or("-"));
    println!("  Language: {}", config.language.as_deref().unwrap_or("-"));
    if let Some(window) = &config.budget.window {
        println!("  Budget window: {}", window);
    }
    if let Some(cost) = config.budget.cost_limit_usd {
        println!("  Budget limit: ${cost:.2}");
    }
    if let Some(tokens) = config.budget.token_limit {
        println!("  Budget token limit: {}", tokens);
    }
    if config.budget.prefer_budget {
        println!("  Budget prefer_budget: true");
    }
    if config.rules.is_empty() {
        println!("  Rules: (none)");
    } else {
        println!("  Rules: {} rule(s)", config.rules.len());
        for rule in &config.rules {
            println!("    - {rule}");
        }
    }
    let knowledge_entries = project::read_project_knowledge(&git_root);
    let knowledge_index = project::project_knowledge_dir(&git_root).join("KNOWLEDGE.md");
    println!("  Knowledge: {} entries ({})", knowledge_entries.len(), knowledge_index.display());
    Ok(())
}
fn write_project_config(
    git_root: &Path,
    project_id: &str,
    profile: &str,
    language: Option<&str>,
) -> Result<(PathBuf, PathBuf)> {
    let aid_dir = git_root.join(".aid");
    let project_path = aid_dir.join("project.toml");
    fs::create_dir_all(&aid_dir)
        .with_context(|| format!("Failed to create {}", aid_dir.display()))?;
    if project_path.exists() {
        bail!("Project config already exists at {}", project_path.display());
    }
    let mut lines = vec!["[project]".to_string(), format!("id = \"{}\"", project_id), format!("profile = \"{}\"", profile)];
    if let Some(lang) = language
        && !lang.trim().is_empty() {
            lines.push(format!("language = \"{}\"", lang.trim()));
        }
    lines.push(String::new());
    fs::write(&project_path, lines.join("\n"))?;
    let knowledge_dir = project::project_knowledge_dir(git_root);
    fs::create_dir_all(&knowledge_dir)
        .with_context(|| format!("Failed to create {}", knowledge_dir.display()))?;
    let knowledge_index = knowledge_dir.join("KNOWLEDGE.md");
    if !knowledge_index.exists() {
        fs::write(
            &knowledge_index,
            format!(
                "# {project_id} — Project Knowledge\n\n<!-- Add knowledge entries as: - [topic](knowledge/file.md) — description -->\n",
            ),
        )?;
    }
    Ok((project_path, knowledge_index))
}
fn prompt_project_id(git_root: &Path) -> Result<String> {
    let default = git_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    prompt_line("Project ID", Some(default), false)
}
fn prompt_profile(label: &str, default: &str) -> Result<String> {
    loop {
        let value = prompt_line(label, Some(default), false)?;
        let normalized = value.to_lowercase();
        match normalized.as_str() {
            "hobby" | "standard" | "production" => return Ok(normalized),
            _ => eprintln!("Allowed profiles: hobby, standard, production."),
        }
    }
}
fn prompt_language(default: Option<&str>) -> Result<Option<String>> {
    let entry = prompt_line("Language", default, true)?;
    if entry.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(entry.trim().to_string()))
    }
}
fn prompt_line(label: &str, default: Option<&str>, allow_empty: bool) -> Result<String> {
    loop {
        match default {
            Some(value) => print!("{label} [{value}]: "),
            None => print!("{label}: "),
        }
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
        if let Some(value) = default {
            return Ok(value.to_string());
        }
        if allow_empty {
            return Ok(String::new());
        }
        eprintln!("{} cannot be empty.", label);
    }
}
fn detect_language(git_root: &Path) -> Option<String> {
    let cargo = git_root.join("Cargo.toml");
    if cargo.is_file() {
        return Some("rust".to_string());
    }
    let package = git_root.join("package.json");
    if package.is_file() {
        if package_json_has_typescript(&package) {
            return Some("typescript".to_string());
        }
        return Some("javascript".to_string());
    }
    None
}

fn package_json_has_typescript(path: &Path) -> bool {
    let raw = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return false,
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return false,
    };
    ["dependencies", "devDependencies", "peerDependencies"].iter().any(|key| {
        parsed
            .get(*key)
            .and_then(|deps| deps.as_object())
            .is_some_and(|deps| deps.contains_key("typescript"))
    })
}
fn current_git_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run `git rev-parse --show-toplevel`")?;
    if !output.status.success() {
        bail!("Not inside a git repository");
    }
    let root = String::from_utf8(output.stdout)
        .context("Failed to read git root from git output")?
        .trim()
        .to_string();
    if root.is_empty() {
        bail!("Git root path is empty");
    }
    Ok(PathBuf::from(root))
}
