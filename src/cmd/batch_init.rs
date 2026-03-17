// Generate a batch TOML template with all available fields.
// Exports: init(); depends on crate::project for auto-detecting project defaults.

use anyhow::Result;

/// Generate a batch TOML template file, pre-filled with project defaults.
pub fn init(output_path: Option<&str>) -> Result<()> {
    let path = output_path.unwrap_or("tasks.toml");
    if std::path::Path::new(path).exists() {
        anyhow::bail!("{path} already exists — use -o <file> to choose a different name");
    }

    let (dir, team, verify, language) = detect_project_defaults();
    let template = render_template(&dir, &team, &verify, &language);

    std::fs::write(path, &template)?;
    println!("[aid] Created {path}");
    println!("[aid] Edit the [[task]] entries, then run: aid batch {path} --parallel");
    Ok(())
}

fn detect_project_defaults() -> (String, String, String, String) {
    let project = crate::project::detect_project();
    match project {
        Some(config) => {
            let dir = ".".to_string();
            let team = config.team.unwrap_or_default();
            let verify = config.verify.unwrap_or_default();
            let language = config.language.unwrap_or_default();
            (dir, team, verify, language)
        }
        None => (String::new(), String::new(), String::new(), String::new()),
    }
}

fn render_template(dir: &str, team: &str, verify: &str, language: &str) -> String {
    let mut lines = Vec::new();
    lines.push("# Batch task file for aid".to_string());
    lines.push("# Docs: aid batch --help".to_string());
    lines.push(String::new());

    // [defaults] section
    lines.push("[defaults]".to_string());
    if dir.is_empty() {
        lines.push("# dir = \".\"                              # Working directory".to_string());
    } else {
        lines.push(format!("dir = \"{dir}\""));
    }
    lines.push("# agent = \"codex\"                        # Default agent for all tasks".to_string());
    if team.is_empty() {
        lines.push("# team = \"<TEAM_ID>\"                     # Team knowledge injection".to_string());
    } else {
        lines.push(format!("team = \"{team}\""));
    }
    if verify.is_empty() {
        lines.push("# verify = \"<VERIFY_CMD>\"                # Auto-verify on completion".to_string());
    } else {
        lines.push(format!("verify = \"{verify}\""));
    }
    lines.push("# fallback = \"cursor\"                    # Agent to try if primary fails".to_string());
    lines.push("# model = \"<MODEL>\"                      # Model override for all tasks".to_string());
    lines.push("# context = [\"src/types.rs\"]             # Files to inject as context".to_string());
    lines.push("# skills = [\"implementer\"]               # Methodology skills".to_string());
    lines.push("# read_only = false                      # Read-only mode".to_string());
    lines.push("# budget = false                         # Budget/cheap mode".to_string());
    lines.push("# max_duration_mins = 30                 # Per-task timeout".to_string());
    lines.push(String::new());

    // First task
    lines.push("[[task]]".to_string());
    lines.push("name = \"task-1\"".to_string());
    lines.push("agent = \"codex\"".to_string());
    lines.push(format!(
        "prompt = \"\"\"",
    ));
    if language.is_empty() {
        lines.push("<DESCRIBE_TASK_HERE>".to_string());
    } else {
        lines.push(format!("Implement <FEATURE> in {language}."));
    }
    lines.push("\"\"\"".to_string());
    lines.push("# worktree = \"feat/<BRANCH>\"             # Git worktree for isolation".to_string());
    lines.push("# fallback = \"cursor\"                    # Fallback agent on failure".to_string());
    lines.push("# context = [\"src/types.rs\"]             # Extra context files".to_string());
    lines.push("# depends_on = [\"other-task\"]            # Run after named task(s)".to_string());
    lines.push("# on_success = \"deploy\"                  # Trigger conditional task on success".to_string());
    lines.push("# on_fail = \"notify\"                     # Trigger conditional task on failure".to_string());
    lines.push(String::new());

    // Second task (example)
    lines.push("# [[task]]".to_string());
    lines.push("# name = \"task-2\"".to_string());
    lines.push("# agent = \"opencode\"".to_string());
    lines.push("# prompt = \"<DESCRIBE_TASK_HERE>\"".to_string());
    lines.push("# depends_on = [\"task-1\"]".to_string());
    lines.push(String::new());

    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_with_no_project() {
        let template = render_template("", "", "", "");
        assert!(template.contains("[defaults]"));
        assert!(template.contains("[[task]]"));
        assert!(template.contains("<DESCRIBE_TASK_HERE>"));
        assert!(template.contains("# fallback"));
        assert!(template.contains("# depends_on"));
    }

    #[test]
    fn render_template_with_project_defaults() {
        let template = render_template(".", "dev", "cargo test", "rust");
        assert!(template.contains("dir = \".\""));
        assert!(template.contains("team = \"dev\""));
        assert!(template.contains("verify = \"cargo test\""));
        assert!(template.contains("Implement <FEATURE> in rust."));
        assert!(!template.contains("# dir ="));
        assert!(!template.contains("# team ="));
    }

    #[test]
    fn init_refuses_existing_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();
        let result = init(Some(path));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn init_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-tasks.toml");
        let path_str = path.to_str().unwrap();
        init(Some(path_str)).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[defaults]"));
        assert!(content.contains("[[task]]"));
    }
}
