// Project command handlers for the `aid project` CLI group.
// Exports: run_project_command.
// Deps: crate::config, crate::config::project, serde_json, std::{fs, io, path, process}.
mod state_command;

mod prompts;
use prompts::*;

use crate::cli::ProjectAction;
use crate::{config as aid_config, config::project};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
pub fn run_project_command(action: ProjectAction) -> Result<()> {
    match action {
        ProjectAction::Init => init(),
        ProjectAction::Show => show(),
        ProjectAction::State => state(),
        ProjectAction::Sync => sync(),
    }
}
fn init() -> Result<()> {
    let git_root = current_git_root()?;
    let project_id = prompt_project_id(&git_root)?;
    let profile = prompt_profile("Profile (hobby/standard/production)", "standard")?;
    let language = prompt_language(detect_language(&git_root).as_deref())?;
    let (budget_shorthand, budget_cost, budget_window) =
        prompt_daily_budget(default_budget_for_profile(&profile))?;
    let team_input = prompt_line("Team (optional)", Some(""), true)?;
    let team = if team_input.is_empty() {
        None
    } else {
        Some(team_input)
    };
    let verify_default = default_verify_for_language(language.as_deref());
    let verify_command = prompt_line("Verify command", Some(verify_default), false)?;
    let gitbutler = detect_gitbutler_mode()?;
    let (project_path, knowledge_index) = write_project_config(
        &git_root,
        &project_id,
        &profile,
        language.as_deref(),
        Some(&budget_shorthand),
        team.as_deref(),
        Some(verify_command.as_str()),
        gitbutler,
    )?;
    aid_config::upsert_budget(&project_id, budget_cost, budget_window.as_deref())?;
    println!("  Budget synced to ~/.aid/config.toml");
    let config = project::load_project(&project_path)?;
    crate::prompt::claudemd::sync_claude_md(&git_root, &config)?;
    println!("  CLAUDE.md updated with aid section");
    println!("Project: {}", config.id);
    println!("  Profile: {}", config.profile.as_deref().unwrap_or("-"));
    println!("  Language: {}", config.language.as_deref().unwrap_or("-"));
    println!("  File: {}", project_path.display());
    println!("  Knowledge: {}", knowledge_index.display());
    Ok(())
}
fn sync() -> Result<()> {
    let git_root = current_git_root()?;
    let config = project::detect_project()
        .ok_or_else(|| anyhow!("No project configuration found. Run `aid project init` first."))?;

    if config.budget.cost_limit_usd.is_some() || config.budget.token_limit.is_some() {
        let window = config.budget.window.as_deref();
        aid_config::upsert_budget_limits(
            &config.id,
            config.budget.cost_limit_usd,
            config.budget.token_limit,
            window,
        )?;
        println!("Budget synced to ~/.aid/config.toml");
    }

    crate::prompt::claudemd::sync_claude_md(&git_root, &config)?;
    println!("CLAUDE.md updated with aid section");

    Ok(())
}
fn show() -> Result<()> {
    let config = project::detect_project().ok_or_else(|| {
        anyhow!("No project configuration found. Run `aid project init` in a git repository.")
    })?;
    let git_root = current_git_root()?;
    println!("Project: {}", config.id);
    println!("  Profile:    {}", config.profile.as_deref().unwrap_or("-"));
    println!("  Team:       {}", config.team.as_deref().unwrap_or("-"));
    println!("  Language:   {}", config.language.as_deref().unwrap_or("-"));
    println!("  Verify:     {}", config.verify.as_deref().unwrap_or("-"));
    println!("  Container:  {}", config.container.as_deref().unwrap_or("-"));
    let gitbutler_display = config
        .gitbutler
        .as_deref()
        .map(|_| config.gitbutler_mode().as_str())
        .unwrap_or("-");
    println!("  GitButler:  {}", gitbutler_display);
    print_project_budget(&config);
    if config.rules.is_empty() {
        println!("  Rules:      (none)");
    } else {
        println!("  Rules:      {} rule(s)", config.rules.len());
        for rule in &config.rules {
            println!("    - {rule}");
        }
    }
    let knowledge_entries = project::read_project_knowledge(&git_root);
    let knowledge_index = project::project_knowledge_dir(&git_root).join("KNOWLEDGE.md");
    println!("  Knowledge:  {} entries", knowledge_entries.len());
    println!("    Index: {}", knowledge_index.display());
    Ok(())
}
fn state() -> Result<()> {
    state_command::run()
}
fn write_project_config(
    git_root: &Path,
    project_id: &str,
    profile: &str,
    language: Option<&str>,
    budget: Option<&str>,
    team: Option<&str>,
    verify: Option<&str>,
    gitbutler: Option<&str>,
) -> Result<(PathBuf, PathBuf)> {
    let aid_dir = git_root.join(".aid");
    let project_path = aid_dir.join("project.toml");
    fs::create_dir_all(&aid_dir).with_context(|| format!("Failed to create {}", aid_dir.display()))?;
    let batches_dir = aid_dir.join("batches");
    if !batches_dir.exists() {
        std::fs::create_dir_all(&batches_dir)?;
        aid_info!("[aid] Created .aid/batches/ for batch TOML files");
    }
    if project_path.exists() {
        bail!("Project config already exists at {}", project_path.display());
    }
    let lines = project_config_lines(project_id, profile, language, budget, team, verify, gitbutler);
    fs::write(&project_path, lines.join("\n"))?;
    let knowledge_index = write_knowledge_index(git_root, project_id)?;
    Ok((project_path, knowledge_index))
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
    ["dependencies", "devDependencies", "peerDependencies"]
        .iter()
        .any(|key| {
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

fn print_project_budget(config: &project::ProjectConfig) {
    let budget_display = if let Some(shorthand) = config.budget.budget_shorthand() {
        format!("{shorthand} (shorthand)")
    } else if let Some(cost) = config.budget.cost_limit_usd {
        let window = config.budget.window.as_deref().unwrap_or("unlimited");
        format!("${cost:.2}/{window}")
    } else {
        "-".to_string()
    };
    println!("  Budget:     {}", budget_display);
    match aid_config::effective_budget(&config.id) {
        Ok(Some((cost, window))) => {
            let window_str = window.as_deref().unwrap_or("unlimited");
            println!("  Effective:  ${cost:.2}/{window_str} (synced to ~/.aid/config.toml)");
        }
        Ok(None) => {
            println!("  Effective:  (not configured in ~/.aid/config.toml)");
        }
        Err(_) => {}
    }
}

fn write_knowledge_index(git_root: &Path, project_id: &str) -> Result<PathBuf> {
    let knowledge_dir = project::project_knowledge_dir(git_root);
    fs::create_dir_all(&knowledge_dir)
        .with_context(|| format!("Failed to create {}", knowledge_dir.display()))?;
    let knowledge_index = knowledge_dir.join("KNOWLEDGE.md");
    if !knowledge_index.exists() {
        fs::write(
            &knowledge_index,
            format!(
                "# {project_id} — Project Knowledge\n\n<!-- Add knowledge entries as: - \
                [topic](knowledge/file.md) — description -->\n",
            ),
        )?;
    }
    Ok(knowledge_index)
}

fn project_config_lines(
    project_id: &str,
    profile: &str,
    language: Option<&str>,
    budget: Option<&str>,
    team: Option<&str>,
    verify: Option<&str>,
    gitbutler: Option<&str>,
) -> Vec<String> {
    let mut lines = vec![
        "[project]".to_string(),
        format!("id = \"{}\"", project_id),
        format!("profile = \"{}\"", profile),
    ];
    if let Some(lang) = language
        && !lang.trim().is_empty()
    {
        lines.push(format!("language = \"{}\"", lang.trim()));
    }
    if let Some(value) = budget
        && !value.trim().is_empty()
    {
        lines.push(format!("budget = \"{}\"", value.trim()));
    }
    if let Some(value) = team
        && !value.trim().is_empty()
    {
        lines.push(format!("team = \"{}\"", value.trim()));
    }
    if let Some(value) = verify
        && !value.trim().is_empty()
    {
        lines.push(format!("verify = \"{}\"", value.trim()));
    }
    if let Some(value) = gitbutler
        && !value.trim().is_empty()
    {
        lines.push(format!("gitbutler = \"{}\"", value.trim()));
    }
    lines.push(String::new());
    lines
}
