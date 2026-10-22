// Toolbox loading, filtering, and scope tests.
// Depends on the toolbox module and temporary filesystem fixtures.

use super::*;
use crate::support::paths;
use std::fs;
use tempfile::TempDir;

#[test]
fn loads_tool_from_toml_file() {
    let temp = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(temp.path());
    let dir = tools_dir();
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("lint.toml"),
        "[tool]\nid = \"lint\"\ndisplay_name = \"Linter\"\ndescription \
            = \"Run linting\"\ncommand = \"eslint\"\nargs = \"--format json\"\noutput_format = \
            \"json\"\ntags = [\"quality\"]\n",
    )
    .unwrap();

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
    fs::write(
        dir.join("check.sh"),
        "#!/bin/bash\n# @description: Run checks\n# @args: <files...>\n# @output: JSON results\n",
    )
    .unwrap();

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
    fs::write(
        dir.join("alpha.toml"),
        "[tool]\nid = \"alpha\"\ncommand = \"alpha-cmd\"\n",
    )
    .unwrap();
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
    fs::write(
        global.join("shared.toml"),
        "[tool]\nid = \"shared\"\ncommand = \"global-cmd\"\ndescription = \"global\"\n",
    )
    .unwrap();

    let team = team_tools_dir("dev");
    fs::create_dir_all(&team).unwrap();
    fs::write(
        team.join("shared.toml"),
        "[tool]\nid = \"shared\"\ncommand = \"team-cmd\"\ndescription = \"team\"\n",
    )
    .unwrap();

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
    assert!(output.contains("lint: Run linting"));
    assert!(
        !output.contains("eslint"),
        "command path should not appear in summary"
    );
    assert!(
        !output.contains("Output: json"),
        "output format should not appear in summary"
    );
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
    fs::write(
        dir.join("minimal.toml"),
        "[tool]\nid = \"minimal\"\ncommand = \"run-it\"\n",
    )
    .unwrap();

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
fn filter_by_task_category_keeps_untagged_tools() {
    let tools = vec![make_tool("universal")];
    let filtered = filter_by_task_category(tools, "research");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "universal");
}

#[test]
fn filter_by_task_category_filters_irrelevant() {
    let mut tool = make_tool("rpc");
    tool.tags = vec!["rpc".to_string()];
    let filtered = filter_by_task_category(vec![tool], "research");
    assert!(filtered.is_empty());
}

#[test]
fn filter_by_task_category_keeps_relevant() {
    let mut tool = make_tool("search");
    tool.tags = vec!["search".to_string()];
    let filtered = filter_by_task_category(vec![tool], "research");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "search");
}

#[test]
fn filter_by_task_category_empty_category_keeps_all() {
    let mut tagged = make_tool("search");
    tagged.tags = vec!["search".to_string()];
    let tools = vec![make_tool("universal"), tagged];
    let filtered = filter_by_task_category(tools, "");
    assert_eq!(filtered.len(), 2);
}

#[test]
fn project_scope_overrides_global() {
    let temp = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(temp.path());
    let global = tools_dir();
    fs::create_dir_all(&global).unwrap();
    fs::write(
        global.join("tool.toml"),
        "[tool]\nid = \"tool\"\ncommand = \"global\"\n",
    )
    .unwrap();

    let project = temp.path().join("project");
    let project_tools = project.join(".aid").join("tools");
    fs::create_dir_all(&project_tools).unwrap();
    fs::write(
        project_tools.join("tool.toml"),
        "[tool]\nid = \"tool\"\ncommand = \"project\"\n",
    )
    .unwrap();

    let tools = resolve_toolbox(None, Some(&project));
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].command, "project");
    assert_eq!(tools[0].scope, ToolScope::Project);
}
