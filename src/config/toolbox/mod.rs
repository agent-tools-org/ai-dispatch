// Team toolbox: loadable CLI tools for prompt injection.
// Exports: ToolMeta, ToolScope, tools_dir, team_tools_dir, list_tools, list_team_tools,
//          resolve_toolbox, filter_by_auto_inject, find_tool, format_toolbox_instructions.
// Deps: crate::support::paths, crate::support::sanitize, anyhow, serde, toml, std::fs.

use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum ToolScope {
    Global,
    Team(String),
    Project,
}

impl ToolScope {
    pub fn label(&self) -> &str {
        match self {
            ToolScope::Global => "global",
            ToolScope::Team(_) => "team",
            ToolScope::Project => "project",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolMeta {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub command: String,
    pub args: String,
    pub output_format: String,
    pub tags: Vec<String>,
    pub scope: ToolScope,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolFile {
    tool: ToolConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolConfig {
    id: String,
    display_name: Option<String>,
    #[serde(default)]
    description: String,
    command: String,
    #[serde(default)]
    args: String,
    #[serde(default = "default_output_format")]
    output_format: String,
    #[serde(default)]
    tags: Vec<String>,
}

fn default_output_format() -> String {
    "text".to_string()
}

pub fn tools_dir() -> PathBuf {
    crate::support::paths::aid_dir().join("tools")
}

pub fn team_tools_dir(team_id: &str) -> PathBuf {
    crate::config::team::teams_dir().join(team_id).join("tools")
}

fn load_tool_from_toml(path: &Path, scope: ToolScope) -> Option<ToolMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    let file: ToolFile = toml::from_str(&content).ok()?;
    let ToolConfig {
        id,
        display_name,
        description,
        command,
        args,
        output_format,
        tags,
    } = file.tool;
    let display_name = display_name.unwrap_or_else(|| id.clone());
    Some(ToolMeta {
        name: id,
        display_name,
        description,
        command,
        args,
        output_format,
        tags,
        scope,
    })
}

fn parse_tool_script(path: &Path, scope: ToolScope) -> Option<ToolMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    let name = path.file_stem()?.to_str()?.to_string();
    let mut description = String::new();
    let mut args = String::new();
    let mut output_hint = String::new();
    for line in content.lines().take(10) {
        let trimmed = line.trim_start_matches('#').trim();
        if let Some(desc) = trimmed.strip_prefix("@description:") {
            description = desc.trim().to_string();
        } else if let Some(a) = trimmed.strip_prefix("@args:") {
            args = a.trim().to_string();
        } else if let Some(o) = trimmed.strip_prefix("@output:") {
            output_hint = o.trim().to_string();
        }
    }
    if description.is_empty() {
        description = format!("Run {name}");
    }
    let output_format = if output_hint.to_lowercase().contains("json") {
        "json".to_string()
    } else {
        "text".to_string()
    };
    Some(ToolMeta {
        display_name: name.clone(),
        name,
        description,
        command: path.to_string_lossy().to_string(),
        args,
        output_format,
        tags: Vec::new(),
        scope,
    })
}

fn list_tools_in_dir(dir: &Path, scope: ToolScope) -> Vec<ToolMeta> {
    let mut tools = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                if let Some(tool) = load_tool_from_toml(&path, scope.clone()) {
                    tools.push(tool);
                }
            }
        }
    }
    let scripts_dir = dir.join("scripts");
    if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
        for entry in entries.flatten() {
            let is_file = entry.file_type().ok().map(|ft| ft.is_file()).unwrap_or(false);
            let visible = entry
                .file_name()
                .to_str()
                .map(|n| !n.starts_with('.'))
                .unwrap_or(false);
            if is_file && visible {
                if let Some(tool) = parse_tool_script(&entry.path(), scope.clone()) {
                    tools.push(tool);
                }
            }
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

#[cfg(test)]
pub fn list_tools() -> Vec<ToolMeta> {
    list_tools_in_dir(&tools_dir(), ToolScope::Global)
}

pub fn list_team_tools(team_id: &str) -> Vec<ToolMeta> {
    list_tools_in_dir(&team_tools_dir(team_id), ToolScope::Team(team_id.to_string()))
}

pub fn resolve_toolbox(team_id: Option<&str>, project_dir: Option<&Path>) -> Vec<ToolMeta> {
    let mut seen: BTreeMap<String, ToolMeta> = BTreeMap::new();
    for tool in list_tools_in_dir(&tools_dir(), ToolScope::Global) {
        seen.insert(tool.name.clone(), tool);
    }
    if let Some(id) = team_id {
        for tool in list_tools_in_dir(&team_tools_dir(id), ToolScope::Team(id.to_string())) {
            seen.insert(tool.name.clone(), tool);
        }
    }
    if let Some(dir) = project_dir {
        for tool in list_tools_in_dir(&dir.join(".aid").join("tools"), ToolScope::Project) {
            seen.insert(tool.name.clone(), tool);
        }
    }
    seen.into_values().collect()
}

pub fn filter_by_auto_inject(tools: Vec<ToolMeta>, auto_inject: &[String]) -> Vec<ToolMeta> {
    if auto_inject.is_empty() {
        return tools;
    }
    let allow: std::collections::HashSet<&str> = auto_inject.iter().map(|s| s.as_str()).collect();
    tools
        .into_iter()
        .filter(|t| allow.contains(t.name.as_str()))
        .collect()
}

pub fn filter_by_task_category(tools: Vec<ToolMeta>, category: &str) -> Vec<ToolMeta> {
    let category = category.trim();
    if category.is_empty() {
        return tools;
    }
    let relevant_tags: &[&str] = match category {
        "research" => &["research", "search", "docs", "web"],
        "simple-edit" | "complex-impl" | "refactoring" => &["build", "lint", "test", "verify", "deploy"],
        "frontend" => &["build", "lint", "test", "ui", "design"],
        "debugging" => &["debug", "trace", "logs", "rpc", "chain"],
        "testing" => &["test", "verify", "build"],
        "documentation" => &["docs", "search", "web"],
        _ => return tools,
    };
    tools
        .into_iter()
        .filter(|tool| {
            tool.tags.is_empty() || tool.tags.iter().any(|tag| relevant_tags.contains(&tag.as_str()))
        })
        .collect()
}

pub fn find_tool(name: &str, team_id: Option<&str>, project_dir: Option<&Path>) -> Result<ToolMeta> {
    crate::support::sanitize::validate_name(name, "tool")?;
    resolve_toolbox(team_id, project_dir)
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| anyhow::anyhow!("Tool '{name}' not found"))
}

pub fn format_toolbox_instructions(tools: &[ToolMeta]) -> String {
    if tools.is_empty() {
        return String::new();
    }
    let mut lines = vec!["--- Team Toolbox ---".to_string()];
    lines.push(
        "The following tools are available via bash. Use `aid tool show <name>` for full usage.".to_string(),
    );
    lines.push(String::new());
    for tool in tools {
        lines.push(format!("  {}: {}", tool.name, tool.description));
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "toolbox_tests.rs"]
mod tests;
