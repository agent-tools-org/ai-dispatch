// Prompt template loading plus shared prompt injections.
// Exports: list_templates(), load_template(), apply_template(), inject_* helpers.
// Deps: crate::paths, anyhow, std::fs.

use anyhow::{Context, Result};

fn templates_dir() -> std::path::PathBuf { crate::paths::aid_dir().join("templates") }

pub fn list_templates() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(templates_dir()) else { return vec![] };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

pub fn load_template(name: &str) -> Result<String> {
    let path = templates_dir().join(format!("{name}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("Template '{name}' not found in ~/.aid/templates/")
        }
        Err(err) => Err(err).with_context(|| format!("Failed to read template {}", path.display())),
    }
}

pub fn apply_template(template_content: &str, user_prompt: &str) -> String {
    template_content.replace("{{prompt}}", user_prompt)
}
pub fn milestone_instruction() -> &'static str { "\nAfter completing each major step, print on its own line: [MILESTONE] <brief description>" }
pub fn inject_milestone_prompt(raw: &str) -> String { format!("{raw}{}", milestone_instruction()) }
pub fn codex_guard() -> &'static str { "\nIMPORTANT: If no changes are needed, do NOT create an empty commit. Instead, print 'NO_CHANGES_NEEDED: <reason>' and exit." }
pub fn codex_commit_msg(msg: &str) -> String { format!("\nCommit with message: '{msg}'") }
pub fn inject_codex_prompt(raw: &str, commit_msg: Option<&str>) -> String { format!("{raw}{}{}", codex_guard(), commit_msg.map(codex_commit_msg).unwrap_or_default()) }

pub fn text_edit_guard(prompt: &str) -> Option<&'static str> {
    let text_extensions = [".md", ".txt", ".toml", ".yaml", ".yml", ".json", ".cfg", ".ini", ".csv"];
    let lower = prompt.to_lowercase();
    if text_extensions.iter().any(|ext| lower.contains(ext)) {
        Some("\nIMPORTANT: When editing text/config files, make targeted edits only. Do NOT rewrite or regenerate entire files. Preserve existing content and structure.\n")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_template_replaces_placeholder() {
        assert_eq!(apply_template("Task:\n{{prompt}}", "fix the failing test"), "Task:\nfix the failing test");
    }

    #[test]
    fn text_edit_guard_triggers_for_md_files() {
        assert!(text_edit_guard("Edit README.md to add a section").is_some());
    }

    #[test]
    fn text_edit_guard_does_not_trigger_for_code() {
        assert!(text_edit_guard("Refactor the login function in auth.rs").is_none());
    }

    #[test]
    fn text_edit_guard_triggers_for_toml() {
        assert!(text_edit_guard("Update Cargo.toml version").is_some());
    }
}
