// Install assets helpers for the store command.
// Exports: command phases used by the parent module.
// Deps: parent command types and supporting configuration.

use super::{AgentEntry, HookEntry, REPO_RAW, curl_fetch};
use crate::config::hooks::Hook;
use crate::support::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::{fs, io};
pub(super) fn install_scripts(entry: &AgentEntry, publisher: &str) -> Result<()> {
    if entry.scripts.is_empty() {
        return Ok(());
    }
    let scripts_dir = paths::aid_dir().join("scripts");
    fs::create_dir_all(&scripts_dir)?;
    for script in &entry.scripts {
        let script_url = format!("{REPO_RAW}/scripts/{publisher}/{script}");
        match curl_fetch(&script_url) {
            Ok(content) => {
                let target = scripts_dir.join(script);
                fs::write(&target, &content)?;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
                println!("  Script: {}", target.display());
            }
            Err(e) => aid_warn!("  Warning: script {script}: {e}"),
        }
    }
    Ok(())
}

pub(super) fn install_skills(entry: &AgentEntry, publisher: &str) -> Result<()> {
    if entry.skills.is_empty() {
        return Ok(());
    }
    let skills_dir = paths::aid_dir().join("skills");
    fs::create_dir_all(&skills_dir)?;
    for skill in &entry.skills {
        let skill_url = format!("{REPO_RAW}/skills/{publisher}/{skill}");
        match curl_fetch(&skill_url) {
            Ok(content) => {
                let target = skills_dir.join(skill);
                fs::write(&target, &content)?;
                println!("  Skill: {}", target.display());
            }
            Err(e) => aid_warn!("  Warning: skill {skill}: {e}"),
        }
    }
    Ok(())
}

pub(super) fn install_hooks(entries: &[HookEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let hooks_path = paths::aid_dir().join("hooks.toml");
    let mut hooks_file = load_hooks_file(&hooks_path)?;
    let mut added = Vec::new();
    for entry in entries {
        if hooks_file
            .hook
            .iter()
            .any(|hook| hook.event == entry.event && hook.command == entry.command)
        {
            continue;
        }
        hooks_file.hook.push(Hook::new_trusted(
            entry.event.clone(),
            entry.command.clone(),
            None,
        ));
        added.push(entry.clone());
    }
    if added.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(paths::aid_dir())?;
    let serialized = toml::to_string(&hooks_file)?;
    fs::write(&hooks_path, serialized)?;
    for entry in added {
        println!("  Hook: {} -> {}", entry.event, entry.command);
    }
    Ok(())
}

fn load_hooks_file(path: &Path) -> Result<HooksFile> {
    match fs::read_to_string(path) {
        Ok(content) if !content.trim().is_empty() => {
            toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
        }
        Ok(_) => Ok(HooksFile::default()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(HooksFile::default()),
        Err(err) => Err(err).with_context(|| format!("Failed to read {}", path.display())),
    }
}

#[derive(Deserialize, Serialize, Default)]
struct HooksFile {
    #[serde(rename = "hook", default)]
    hook: Vec<Hook>,
}
