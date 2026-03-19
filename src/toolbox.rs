// Team toolbox: loadable CLI tools for prompt injection.
// Exports: ToolMeta, ToolScope, tools_dir, team_tools_dir, list_tools, list_team_tools,
//          resolve_toolbox, filter_by_auto_inject, find_tool, format_toolbox_instructions.
// Deps: crate::paths, crate::sanitize, anyhow, serde, toml, std::fs.

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
    crate::paths::aid_dir().join("tools")
}

pub fn team_tools_dir(team_id: &str) -> PathBuf {
    crate::team::teams_dir().join(team_id).join("tools")
}

fn load_tool_from_toml(path: &Path, scope: ToolScope) -> Option<ToolMeta> {
    let content = std::fs::read_to_string(path).ok()?;
    let file: ToolFile = toml::from_str(&content).ok()?;
    let ToolConfig { id, display_name, description, command, args, output_format, tags } = file.tool;
    let display_name = display_name.unwrap_or_else(|| id.clone());
    Some(ToolMeta { name: id, display_name, description, command, args, output_format, tags, scope })
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
            let visible = entry.file_name().to_str().map(|n| !n.starts_with('.')).unwrap_or(false);
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
    tools.into_iter().filter(|t| allow.contains(t.name.as_str())).collect()
}

pub fn find_tool(name: &str, team_id: Option<&str>, project_dir: Option<&Path>) -> Result<ToolMeta> {
    crate::sanitize::validate_name(name, "tool")?;
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
    lines.push("The following tools are available. Run them directly via bash:".to_string());
    lines.push(String::new());
    for tool in tools {
        let args_part = if tool.args.is_empty() {
            String::new()
        } else {
            format!(" {}", tool.args)
        };
        lines.push(format!("  {}{}: {}", tool.command, args_part, tool.description));
        if tool.output_format != "text" {
            lines.push(format!("    Output: {}", tool.output_format));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn loads_tool_from_toml_file() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let dir = tools_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("lint.toml"), "[tool]\nid = \"lint\"\ndisplay_name = \"Linter\"\ndescription = \"Run linting\"\ncommand = \"eslint\"\nargs = \"--format json\"\noutput_format = \"json\"\ntags = [\"quality\"]\n").unwrap();

        let tools = list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "lint");
        assert_eq!(tools[0].display_name, "Linter");
        assert_eq!(tools[0].command, "eslint");
        assert_eq!(tools[0].output_format, "json");
        assert_eq!(tools[0].tags, vec!["quality"]);
        assert_eq!(tools[0].scope, ToolScope::Global);
    }

    #[test]
    fn parses_tool_script_metadata() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let dir = tools_dir().join("scripts");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("check.sh"), "#!/bin/bash\n# @description: Run checks\n# @args: <files...>\n# @output: JSON results\n").unwrap();

        let tools = list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "check");
        assert_eq!(tools[0].description, "Run checks");
        assert_eq!(tools[0].args, "<files...>");
        assert_eq!(tools[0].output_format, "json");
    }

    #[test]
    fn list_finds_toml_and_scripts() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let dir = tools_dir();
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("alpha.toml"), "[tool]\nid = \"alpha\"\ncommand = \"alpha-cmd\"\n").unwrap();
        fs::write(dir.join("scripts").join("beta.sh"), "#!/bin/bash\n").unwrap();

        let names: Vec<_> = list_tools().iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn resolve_deduplicates_by_scope_priority() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let global = tools_dir();
        fs::create_dir_all(&global).unwrap();
        fs::write(global.join("shared.toml"), "[tool]\nid = \"shared\"\ncommand = \"global-cmd\"\ndescription = \"global\"\n").unwrap();

        let team = team_tools_dir("dev");
        fs::create_dir_all(&team).unwrap();
        fs::write(team.join("shared.toml"), "[tool]\nid = \"shared\"\ncommand = \"team-cmd\"\ndescription = \"team\"\n").unwrap();

        let tools = resolve_toolbox(Some("dev"), None);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].command, "team-cmd");
        assert_eq!(tools[0].scope, ToolScope::Team("dev".to_string()));
    }

    #[test]
    fn format_instructions_renders_tools() {
        let tools = vec![ToolMeta {
            name: "lint".to_string(),
            display_name: "Linter".to_string(),
            description: "Run linting".to_string(),
            command: "eslint".to_string(),
            args: "<files>".to_string(),
            output_format: "json".to_string(),
            tags: vec![],
            scope: ToolScope::Global,
        }];
        let output = format_toolbox_instructions(&tools);
        assert!(output.contains("--- Team Toolbox ---"));
        assert!(output.contains("eslint <files>: Run linting"));
        assert!(output.contains("Output: json"));
    }

    #[test]
    fn format_empty_returns_empty() {
        assert!(format_toolbox_instructions(&[]).is_empty());
    }

    #[test]
    fn find_tool_rejects_invalid_name() {
        let err = find_tool("../escape", None, None).unwrap_err();
        assert!(err.to_string().contains("Invalid tool name"));
    }

    #[test]
    fn tool_scope_labels() {
        assert_eq!(ToolScope::Global.label(), "global");
        assert_eq!(ToolScope::Team("dev".to_string()).label(), "team");
        assert_eq!(ToolScope::Project.label(), "project");
    }

    #[test]
    fn empty_dir_returns_no_tools() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        assert!(list_tools().is_empty());
    }

    #[test]
    fn toml_defaults_when_optional_fields_omitted() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let dir = tools_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("minimal.toml"), "[tool]\nid = \"minimal\"\ncommand = \"run-it\"\n").unwrap();

        let tools = list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].display_name, "minimal");
        assert_eq!(tools[0].output_format, "text");
        assert!(tools[0].tags.is_empty());
    }

    #[test]
    fn resolve_toolbox_returns_all_without_filter() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let dir = tools_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.toml"), "[tool]\nid = \"a\"\ncommand = \"a-cmd\"\n").unwrap();
        fs::write(dir.join("b.toml"), "[tool]\nid = \"b\"\ncommand = \"b-cmd\"\n").unwrap();

        let tools = resolve_toolbox(None, None);
        assert_eq!(tools.len(), 2);
    }

    fn make_tool(name: &str) -> ToolMeta {
        ToolMeta {
            name: name.to_string(),
            display_name: name.to_string(),
            description: String::new(),
            command: name.to_string(),
            args: String::new(),
            output_format: "text".to_string(),
            tags: vec![],
            scope: ToolScope::Global,
        }
    }

    #[test]
    fn filter_by_auto_inject_filters_correctly() {
        let tools = vec![make_tool("lint"), make_tool("test"), make_tool("build")];
        let filtered = filter_by_auto_inject(tools, &["lint".to_string(), "build".to_string()]);
        assert_eq!(filtered.len(), 2);
        let names: Vec<_> = filtered.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"lint"));
        assert!(names.contains(&"build"));
        assert!(!names.contains(&"test"));
    }

    #[test]
    fn filter_by_auto_inject_empty_returns_all() {
        let tools = vec![make_tool("lint"), make_tool("test")];
        let result = filter_by_auto_inject(tools, &[]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn project_scope_overrides_global() {
        let temp = TempDir::new().unwrap();
        let _guard = paths::AidHomeGuard::set(temp.path());
        let global = tools_dir();
        fs::create_dir_all(&global).unwrap();
        fs::write(global.join("tool.toml"), "[tool]\nid = \"tool\"\ncommand = \"global\"\n").unwrap();

        let project = temp.path().join("project");
        let project_tools = project.join(".aid").join("tools");
        fs::create_dir_all(&project_tools).unwrap();
        fs::write(project_tools.join("tool.toml"), "[tool]\nid = \"tool\"\ncommand = \"project\"\n").unwrap();

        let tools = resolve_toolbox(None, Some(&project));
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].command, "project");
        assert_eq!(tools[0].scope, ToolScope::Project);
    }
}
