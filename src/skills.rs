// Skill loading for methodology prompt injection.
// Exports: load_skill(), load_skill_gotchas(), list_skill_scripts(), list_skill_references().
// Deps: crate::paths, crate::types, anyhow, std::fs.

use anyhow::{Context, Result};
use crate::types::AgentKind;
use crate::sanitize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScriptMeta {
    pub name: String,
    pub path: PathBuf,
    pub description: String,
    pub args: String,
    pub output: String,
}

fn skills_dir() -> std::path::PathBuf {
    crate::paths::aid_dir().join("skills")
}

fn skill_dir(name: &str) -> PathBuf {
    skills_dir().join(name)
}

fn folder_skill_path(name: &str) -> PathBuf {
    skill_dir(name).join("SKILL.md")
}

fn flat_skill_path(name: &str) -> PathBuf {
    skills_dir().join(format!("{name}.md"))
}

fn read_skill_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read skill {}", path.display()))
}

fn list_skill_files(name: &str, subdir: &str) -> Vec<String> {
    let dir = skill_dir(name).join(subdir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_file())
                .map(|_| entry.path().display().to_string())
        })
        .collect();
    paths.sort();
    paths
}

fn read_optional_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn parse_script_metadata(path: &Path) -> Option<ScriptMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    let name = path.file_stem()?.to_str()?.to_string();
    let mut description = String::new();
    let mut args = String::new();
    let mut output = String::new();
    for line in content.lines().take(10) {
        let trimmed = line.trim_start_matches('#').trim();
        if let Some(desc) = trimmed.strip_prefix("@description:") {
            description = desc.trim().to_string();
        } else if let Some(script_args) = trimmed.strip_prefix("@args:") {
            args = script_args.trim().to_string();
        } else if let Some(script_output) = trimmed.strip_prefix("@output:") {
            output = script_output.trim().to_string();
        }
    }
    if description.is_empty() {
        description = format!("Run {name} script");
    }
    Some(ScriptMeta {
        name,
        path: path.to_path_buf(),
        description,
        args,
        output,
    })
}

fn all_agent_kinds() -> &'static [AgentKind] {
    AgentKind::ALL
}

pub fn load_skill(name: &str) -> Result<String> {
    sanitize::validate_name(name, "skill")?;
    let folder_path = folder_skill_path(name);
    if folder_path.is_file() {
        return read_skill_file(&folder_path);
    }
    let flat_path = flat_skill_path(name);
    if flat_path.is_file() {
        return read_skill_file(&flat_path);
    }
    anyhow::bail!("Skill '{name}' not found in ~/.aid/skills/")
}

pub fn resolve_skill_content(name: &str) -> Result<String> {
    load_skill(name)
}

pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

pub fn measure_skill_tokens(name: &str) -> Result<(String, usize)> {
    let content = load_skill(name)?;
    let mut parts = vec![content.clone()];
    let mut gotchas = Vec::new();
    if let Some(general) = read_optional_file(&skill_dir(name).join("gotchas.md")) {
        gotchas.push(general);
    }
    for agent in all_agent_kinds() {
        if let Some(agent_gotchas) =
            read_optional_file(&skill_dir(name).join("gotchas").join(format!("{}.md", agent.as_str())))
        {
            gotchas.push(agent_gotchas);
        }
    }
    if !gotchas.is_empty() {
        parts.push(gotchas.join("\n\n"));
    }
    let scripts = list_skill_scripts(name);
    if !scripts.is_empty() {
        parts.push(scripts.join("\n"));
    }
    let tokens = estimate_tokens(&parts.join("\n\n"));
    Ok((content, tokens))
}

pub fn list_skills() -> Result<Vec<String>> {
    let dir = skills_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut skills = BTreeSet::new();
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read skills dir {}", dir.display()))?
    {
        let path = entry?.path();
        if path.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("md")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            skills.insert(name.to_string());
        }
        if path.is_dir()
            && path.join("SKILL.md").is_file()
            && let Some(name) = path.file_name().and_then(|dir_name| dir_name.to_str())
        {
            skills.insert(name.to_string());
        }
    }
    Ok(skills.into_iter().collect())
}

pub fn load_skill_gotchas(name: &str, agent: &AgentKind) -> Option<String> {
    sanitize::validate_name(name, "skill").ok()?;
    let mut parts = Vec::new();
    if let Some(general) = read_optional_file(&skill_dir(name).join("gotchas.md")) {
        parts.push(general);
    }
    if let Some(agent_specific) = read_optional_file(
        &skill_dir(name)
            .join("gotchas")
            .join(format!("{}.md", agent.as_str())),
    ) {
        parts.push(agent_specific);
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

pub fn list_skill_scripts(name: &str) -> Vec<String> {
    if sanitize::validate_name(name, "skill").is_err() {
        return Vec::new();
    }
    list_skill_files(name, "scripts")
}

pub fn load_skill_scripts(name: &str) -> Vec<ScriptMeta> {
    if sanitize::validate_name(name, "skill").is_err() {
        return Vec::new();
    }
    let dir = skill_dir(name).join("scripts");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut scripts: Vec<ScriptMeta> = entries
        .flatten()
        .filter(|entry| {
            entry.file_type().ok().map(|ft| ft.is_file()).unwrap_or(false)
                && entry.file_name().to_str().map(|name| !name.starts_with('.')).unwrap_or(false)
        })
        .filter_map(|entry| parse_script_metadata(&entry.path()))
        .collect();
    scripts.sort_by(|a, b| a.name.cmp(&b.name));
    scripts
}

pub fn format_script_instructions(scripts: &[ScriptMeta]) -> String {
    if scripts.is_empty() {
        return String::new();
    }
    let mut lines = vec!["--- Available Tools ---".to_string()];
    lines.push("Run these scripts directly via bash. They are pre-installed and executable:".to_string());
    lines.push(String::new());
    for script in scripts {
        let args_part = if script.args.is_empty() {
            String::new()
        } else {
            format!(" {}", script.args)
        };
        lines.push(format!("  {}{}: {}", script.path.display(), args_part, script.description));
        if !script.output.is_empty() {
            lines.push(format!("    Output: {}", script.output));
        }
    }
    lines.join("\n")
}

pub fn list_skill_references(name: &str) -> Vec<String> {
    if sanitize::validate_name(name, "skill").is_err() {
        return Vec::new();
    }
    list_skill_files(name, "references")
}

pub fn auto_skills(agent: &AgentKind, has_worktree: bool) -> Vec<String> {
    let _ = has_worktree;
    let available = list_skills().unwrap_or_default();
    let mut skills = Vec::new();
    match agent {
        AgentKind::Codex
        | AgentKind::Copilot
        | AgentKind::Claude
        | AgentKind::OpenCode
        | AgentKind::Kilo
        | AgentKind::Codebuff
        | AgentKind::Droid
        | AgentKind::Oz => {
            skills.push("implementer".to_string());
        }
        AgentKind::Gemini => {
            skills.push("researcher".to_string());
        }
        AgentKind::Cursor | AgentKind::Custom => {}
    }
    skills.retain(|skill| available.iter().any(|available_skill| available_skill == skill));
    skills
}

#[cfg(test)]
#[path = "skills/tests.rs"]
mod tests;
