// Targeted `.aid/project.toml` editing helpers for batch-time GitButler prompts.
// Exports minimal upsert helpers without rewriting unrelated project config content.
// Deps: super::project_path_in_repo, anyhow, std::{fs, path}.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn upsert_gitbutler_mode(repo_root: &Path, mode: &str) -> Result<PathBuf> {
    upsert_project_setting(repo_root, "gitbutler", &format!("\"{}\"", mode.trim()))
}

pub fn upsert_gitbutler_prompt_suppressed(repo_root: &Path, suppressed: bool) -> Result<PathBuf> {
    let value = if suppressed { "true" } else { "false" };
    upsert_project_setting(repo_root, "suppress_gitbutler_prompt", value)
}

fn upsert_project_setting(repo_root: &Path, key: &str, value: &str) -> Result<PathBuf> {
    let project_path = super::project_path_in_repo(repo_root);
    let aid_dir = project_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo_root.join(".aid"));
    fs::create_dir_all(&aid_dir)
        .with_context(|| format!("Failed to create {}", aid_dir.display()))?;
    let contents = if project_path.exists() {
        fs::read_to_string(&project_path)
            .with_context(|| format!("Failed to read {}", project_path.display()))?
    } else {
        format!("[project]\nid = \"{}\"\n", default_project_id(repo_root))
    };
    let updated = upsert_project_setting_in_contents(&contents, key, value);
    fs::write(&project_path, updated)
        .with_context(|| format!("Failed to write {}", project_path.display()))?;
    Ok(project_path)
}

fn upsert_project_setting_in_contents(contents: &str, key: &str, value: &str) -> String {
    let new_line = format!("{key} = {value}");
    let lines: Vec<&str> = contents.lines().collect();
    let section_idx = lines.iter().position(|line| line.trim() == "[project]");
    let Some(section_idx) = section_idx else {
        let mut updated = format!("[project]\n{new_line}\n");
        if !contents.trim().is_empty() {
            updated.push('\n');
            updated.push_str(contents);
            if !contents.ends_with('\n') {
                updated.push('\n');
            }
        }
        return updated;
    };
    let insert_idx = lines
        .iter()
        .enumerate()
        .skip(section_idx + 1)
        .find(|(_, line)| line.trim_start().starts_with('['))
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    let mut updated = Vec::with_capacity(lines.len() + 1);
    let mut replaced = false;
    for (index, line) in lines.iter().enumerate() {
        if index > section_idx
            && index < insert_idx
            && line.trim_start().starts_with(&format!("{key} ="))
        {
            if !replaced {
                updated.push(new_line.clone());
                replaced = true;
            }
            continue;
        }
        updated.push((*line).to_string());
    }
    if !replaced {
        updated.insert(insert_idx, new_line);
    }
    let mut rendered = updated.join("\n");
    rendered.push('\n');
    rendered
}

fn default_project_id(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("project")
        .replace('"', "")
}
