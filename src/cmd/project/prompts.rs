// Prompts helpers for the project command.
// Exports: command phases used by the parent module.
// Deps: parent command types and supporting configuration.

use anyhow::{Result, anyhow, bail};
use std::io::{self, Write};
use std::path::Path;
pub(super) fn prompt_project_id(git_root: &Path) -> Result<String> {
    let default = git_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    prompt_line("Project ID", Some(default), false)
}
pub(super) fn prompt_profile(label: &str, default: &str) -> Result<String> {
    loop {
        let value = prompt_line(label, Some(default), false)?;
        let normalized = value.to_lowercase();
        match normalized.as_str() {
            "hobby" | "standard" | "production" => return Ok(normalized),
            _ => aid_error!("Allowed profiles: hobby, standard, production."),
        }
    }
}
pub(super) fn prompt_language(default: Option<&str>) -> Result<Option<String>> {
    let entry = prompt_line("Language", default, true)?;
    if entry.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(entry.trim().to_string()))
    }
}

pub(super) fn prompt_daily_budget(default_cost: f64) -> Result<(String, f64, Option<String>)> {
    let default_label = format_budget_default_label(default_cost);
    loop {
        let entry = prompt_line("Daily budget", Some(&default_label), false)?;
        match normalize_budget_input(&entry, "day") {
            Ok(parsed) => return Ok(parsed),
            Err(err) => aid_error!("Invalid budget: {err}"),
        }
    }
}

pub(super) fn default_budget_for_profile(profile: &str) -> f64 {
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
        Some(part) if !part.is_empty() => match part.to_lowercase().as_str() {
            "day" | "daily" => Some("daily".to_string()),
            "month" | "monthly" => Some("monthly".to_string()),
            other => bail!("unsupported budget window '{other}'"),
        },
        Some(_) => bail!("budget window is empty"),
        None => None,
    };
    Ok((cost_limit, window))
}

pub(super) fn default_verify_for_language(language: Option<&str>) -> &'static str {
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

pub(super) fn detect_gitbutler_mode() -> Result<Option<&'static str>> {
    if crate::workspace::gitbutler::but_available() {
        if prompt_confirm("Enable GitButler auto-commit/oplog? [Y/n]", true)? {
            return Ok(Some("auto"));
        }
        return Ok(None);
    }

    println!(
        "  GitButler not found. Install: https://gitbutler.com (CLI provides auto-commit + per-task \
        lanes). Re-run aid project init after install."
    );
    Ok(None)
}

fn prompt_confirm(label: &str, default_yes: bool) -> Result<bool> {
    loop {
        print!("{label}: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default_yes),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => aid_error!("Please answer yes or no."),
        }
    }
}
pub(super) fn prompt_line(label: &str, default: Option<&str>, allow_empty: bool) -> Result<String> {
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
        aid_error!("{} cannot be empty.", label);
    }
}
