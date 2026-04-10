// CLAUDE.md aid section management — inject/update aid config into project CLAUDE.md.
// Exports: sync_claude_md, read_aid_section.
// Deps: crate::project::ProjectConfig, std::{fs, path}.

use crate::project::ProjectConfig;
use anyhow::Result;
use std::fs;
use std::path::Path;

const START_MARKER: &str = "<!-- aid:start -->";
const END_MARKER: &str = "<!-- aid:end -->";

pub fn sync_claude_md(git_root: &Path, config: &ProjectConfig) -> Result<()> {
    let path = git_root.join("CLAUDE.md");
    let section = generate_aid_section(config);
    let next = match fs::read_to_string(&path) {
        Ok(contents) => match marker_range(&contents) {
            Some((start, end)) => {
                let mut updated = String::new();
                updated.push_str(&contents[..start]);
                updated.push_str(&section);
                updated.push_str(&contents[end..]);
                updated
            }
            None if contents.is_empty() => section,
            None => format!("{contents}\n\n{section}"),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => section,
        Err(err) => return Err(err.into()),
    };
    fs::write(path, next)?;
    Ok(())
}

pub fn read_aid_section(git_root: &Path) -> Option<String> {
    let path = git_root.join("CLAUDE.md");
    let contents = fs::read_to_string(path).ok()?;
    let (start, end) = marker_range(&contents)?;
    Some(contents[start..end].to_string())
}

fn generate_aid_section(config: &ProjectConfig) -> String {
    let mut lines = vec![
        START_MARKER.to_string(),
        "## aid orchestration".to_string(),
        String::new(),
        "This project uses [aid](https://github.com/agent-tools-org/ai-dispatch) as the primary development method.".to_string(),
        "Use `aid run` to dispatch coding tasks to AI agents instead of writing code directly.".to_string(),
        String::new(),
    ];
    push_value(&mut lines, "Project", Some(config.id.as_str()));
    push_value(&mut lines, "Profile", config.profile.as_deref());
    push_value(&mut lines, "Language", config.language.as_deref());
    push_value(&mut lines, "Budget", config.budget.budget_shorthand().as_deref());
    push_value(&mut lines, "Verify", config.verify.as_deref());
    push_value(&mut lines, "Team", config.team.as_deref());
    let rules: Vec<_> = config.rules.iter().map(String::as_str).filter(|v| !v.trim().is_empty()).collect();
    if !rules.is_empty() {
        lines.push(String::new());
        lines.push("### Rules".to_string());
        lines.extend(rules.into_iter().map(|rule| format!("- {rule}")));
    }
    lines.push(String::new());
    lines.push("### Usage".to_string());
    lines.push("- Dispatch work: `aid run <agent> \"<prompt>\" --dir .`".to_string());
    lines.push("- Review output: `aid show <id> --diff`".to_string());
    lines.push("- Batch dispatch: `aid batch <file> --parallel`".to_string());
    lines.push("- Project config: `.aid/project.toml`".to_string());
    lines.push(String::new());
    lines.push(END_MARKER.to_string());
    lines.join("\n") + "\n"
}

fn push_value(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
        lines.push(format!("- **{label}**: {value}"));
    }
}

fn marker_range(contents: &str) -> Option<(usize, usize)> {
    let start = contents.find(START_MARKER)?;
    let end_marker = contents[start..].find(END_MARKER)? + start;
    let end = end_marker + END_MARKER.len();
    let end = if contents[end..].starts_with('\n') { end + 1 } else { end };
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config() -> ProjectConfig {
        ProjectConfig {
            id: "alpha".to_string(),
            profile: Some("standard".to_string()),
            team: Some("dev".to_string()),
            verify: Some("cargo check".to_string()),
            gitbutler: None,
            language: Some("rust".to_string()),
            rules: vec!["All changes must have tests".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn sync_creates_new_claude_md() {
        let dir = TempDir::new().unwrap();
        sync_claude_md(dir.path(), &config()).unwrap();
        let body = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(body.contains(START_MARKER));
        assert!(body.contains("- **Project**: alpha"));
    }

    #[test]
    fn sync_updates_existing_section() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "<!-- aid:start -->\nold\n<!-- aid:end -->\n").unwrap();
        let mut cfg = config();
        cfg.id = "beta".to_string();
        sync_claude_md(dir.path(), &cfg).unwrap();
        let body = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(!body.contains("\nold\n"));
        assert!(body.contains("- **Project**: beta"));
    }

    #[test]
    fn sync_appends_to_existing() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "# Notes").unwrap();
        sync_claude_md(dir.path(), &config()).unwrap();
        let body = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(body.starts_with("# Notes\n\n"));
        assert!(body.contains(START_MARKER));
    }

    #[test]
    fn sync_preserves_surrounding_content() {
        let dir = TempDir::new().unwrap();
        let initial = "before\n<!-- aid:start -->\nold\n<!-- aid:end -->\nafter\n";
        fs::write(dir.path().join("CLAUDE.md"), initial).unwrap();
        sync_claude_md(dir.path(), &config()).unwrap();
        let body = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(body.starts_with("before\n"));
        assert!(body.ends_with("after\n"));
    }

    #[test]
    fn read_returns_none_without_file() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read_aid_section(dir.path()), None);
    }

    #[test]
    fn read_returns_section() {
        let dir = TempDir::new().unwrap();
        let body = "x\n<!-- aid:start -->\nsection\n<!-- aid:end -->\ny\n";
        fs::write(dir.path().join("CLAUDE.md"), body).unwrap();
        assert_eq!(read_aid_section(dir.path()), Some("<!-- aid:start -->\nsection\n<!-- aid:end -->\n".to_string()));
    }

    #[test]
    fn optional_fields_omitted() {
        let dir = TempDir::new().unwrap();
        let cfg = ProjectConfig { id: "alpha".to_string(), ..Default::default() };
        sync_claude_md(dir.path(), &cfg).unwrap();
        let body = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(!body.contains("### Rules"));
        assert!(!body.contains("- **Team**:"));
    }
}
