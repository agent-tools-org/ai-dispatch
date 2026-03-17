// Project command handlers for the `aid project` CLI group.
// Exports: ProjectAction, run_project_command.
// Deps: crate::config, crate::project, serde_json, std::{fs, io, path, process}.
use crate::{config as aid_config, project};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
pub enum ProjectAction {
    Init,
    Show,
    Sync,
}
pub fn run_project_command(action: ProjectAction) -> Result<()> {
    match action {
        ProjectAction::Init => init(),
        ProjectAction::Show => show(),
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
    let (project_path, knowledge_index) = write_project_config(
        &git_root,
        &project_id,
        &profile,
        language.as_deref(),
        Some(&budget_shorthand),
        team.as_deref(),
        Some(verify_command.as_str()),
    )?;
    aid_config::upsert_budget(&project_id, budget_cost, budget_window.as_deref())?;
    println!("  Budget synced to ~/.aid/config.toml");
    let config = project::load_project(&project_path)?;
    crate::claudemd::sync_claude_md(&git_root, &config)?;
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

    if let Some(cost) = config.budget.cost_limit_usd {
        let window = config.budget.window.as_deref();
        aid_config::upsert_budget(&config.id, cost, window)?;
        println!("Budget synced to ~/.aid/config.toml");
    }

    crate::claudemd::sync_claude_md(&git_root, &config)?;
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
            println!(
                "  Effective:  ${cost:.2}/{window_str} (synced to ~/.aid/config.toml)"
            );
        }
        Ok(None) => {
            println!("  Effective:  (not configured in ~/.aid/config.toml)");
        }
        Err(_) => {}
    }
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
fn write_project_config(
    git_root: &Path,
    project_id: &str,
    profile: &str,
    language: Option<&str>,
    budget: Option<&str>,
    team: Option<&str>,
    verify: Option<&str>,
) -> Result<(PathBuf, PathBuf)> {
    let aid_dir = git_root.join(".aid");
    let project_path = aid_dir.join("project.toml");
    fs::create_dir_all(&aid_dir)
        .with_context(|| format!("Failed to create {}", aid_dir.display()))?;
    let batches_dir = aid_dir.join("batches");
    if !batches_dir.exists() {
        std::fs::create_dir_all(&batches_dir)?;
        eprintln!("[aid] Created .aid/batches/ for batch TOML files");
    }
    if project_path.exists() {
        bail!("Project config already exists at {}", project_path.display());
    }
    let mut lines = vec![
        "[project]".to_string(),
        format!("id = \"{}\"", project_id),
        format!("profile = \"{}\"", profile),
    ];
    if let Some(lang) = language && !lang.trim().is_empty() {
        lines.push(format!("language = \"{}\"", lang.trim()));
    }
    if let Some(value) = budget && !value.trim().is_empty() {
        lines.push(format!("budget = \"{}\"", value.trim()));
    }
    if let Some(value) = team && !value.trim().is_empty() {
        lines.push(format!("team = \"{}\"", value.trim()));
    }
    if let Some(value) = verify && !value.trim().is_empty() {
        lines.push(format!("verify = \"{}\"", value.trim()));
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

fn prompt_daily_budget(default_cost: f64) -> Result<(String, f64, Option<String>)> {
    let default_label = format_budget_default_label(default_cost);
    loop {
        let entry = prompt_line("Daily budget", Some(&default_label), false)?;
        match normalize_budget_input(&entry, "day") {
            Ok(parsed) => return Ok(parsed),
            Err(err) => eprintln!("Invalid budget: {err}"),
        }
    }
}

fn default_budget_for_profile(profile: &str) -> f64 {
    match profile {
        "hobby" => 5.0,
        "standard" => 20.0,
        "production" => 50.0,
        _ => 20.0,
    }
}

fn format_budget_default_label(cost: f64) -> String {
    let amount = if (cost - cost.trunc()).abs() < f64::EPSILON {
        format!("{:.0}", cost)
    } else {
        format!("{cost}")
    };
    format!("${amount}")
}

fn normalize_budget_input(value: &str, default_window: &str) -> Result<(String, f64, Option<String>)> {
    let mut sanitized = value.trim().to_string();
    if sanitized.is_empty() {
        bail!("Budget cannot be empty");
    }
    if !sanitized.starts_with('$') {
        sanitized.insert(0, '$');
    }
    if !sanitized.contains('/') {
        sanitized.push('/');
        sanitized.push_str(default_window);
    }
    let (cost, window) = parse_budget_value(&sanitized)?;
    Ok((sanitized, cost, window))
}

fn parse_budget_value(value: &str) -> Result<(f64, Option<String>)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("budget shorthand is empty");
    }
    let amount_window = trimmed.strip_prefix('$').unwrap_or(trimmed).trim();
    if amount_window.is_empty() {
        bail!("budget amount is missing");
    }
    let (amount_part, window_part) = match amount_window.split_once('/') {
        Some((left, right)) => (left.trim(), Some(right.trim())),
        None => (amount_window, None),
    };
    if amount_part.is_empty() {
        bail!("budget amount is missing");
    }
    let cost_limit = amount_part
        .parse::<f64>()
        .map_err(|_| anyhow!("invalid budget amount '{amount_part}'"))?;
    let window = match window_part {
        Some(part) if !part.is_empty() => {
            match part.to_lowercase().as_str() {
                "day" | "daily" => Some("daily".to_string()),
                "month" | "monthly" => Some("monthly".to_string()),
                other => bail!("unsupported budget window '{other}'"),
            }
        }
        Some(_) => bail!("budget window is empty"),
        None => None,
    };
    Ok((cost_limit, window))
}

fn default_verify_for_language(language: Option<&str>) -> &'static str {
    match language {
        Some(lang) => {
            let lower = lang.to_ascii_lowercase();
            match lower.as_str() {
                "typescript" | "javascript" | "node" => "npm test",
                _ => "cargo test",
            }
        }
        None => "cargo test",
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
