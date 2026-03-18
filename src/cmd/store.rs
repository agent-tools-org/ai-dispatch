// Handler for `aid store` subcommands — browse, install, show, update agents from the community store.
// Exports: StoreAction, run_store.
// Deps: serde, serde_json, toml, std::fs, std::process::Command (curl), crate::paths.

use anyhow::{bail, Context, Result};
use crate::cmd::store_lock::{add_lock_entry, read_lockfile};
use crate::hooks::Hook;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const REPO_RAW: &str = "https://raw.githubusercontent.com/agent-tools-org/aid-agents/main";

#[derive(Deserialize)]
struct AgentIndex {
    agents: Vec<AgentEntry>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct AgentEntry {
    id: String,
    display_name: String,
    description: String,
    version: String,
    command: String,
    #[serde(default)]
    scripts: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    hooks: Vec<HookEntry>,
    #[serde(default)]
    kind: PackageKind,
}

#[derive(Deserialize, Clone)]
struct HookEntry {
    event: String,
    command: String,
}

#[derive(Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PackageKind {
    #[default]
    Agent,
    Package,
}

pub enum StoreAction {
    Browse { query: Option<String> },
    Install { name: String },
    Show { name: String },
    Update { apply: bool },
}

pub fn run_store(action: StoreAction) -> Result<()> {
    match action {
        StoreAction::Browse { query } => browse(query.as_deref()),
        StoreAction::Install { name } => install(&name),
        StoreAction::Show { name } => show(&name),
        StoreAction::Update { apply } => update(apply),
    }
}

fn browse(query: Option<&str>) -> Result<()> {
    let index = fetch_index()?;

    let agents: Vec<&AgentEntry> = match query {
        Some(q) => {
            let q = q.to_lowercase();
            index
                .agents
                .iter()
                .filter(|a| {
                    a.id.to_lowercase().contains(&q)
                        || a.display_name.to_lowercase().contains(&q)
                        || a.description.to_lowercase().contains(&q)
                })
                .collect()
        }
        None => index.agents.iter().collect(),
    };

    if agents.is_empty() {
        if let Some(q) = query {
            println!("No agents matching \"{q}\".");
        } else {
            println!("Store is empty.");
        }
        return Ok(());
    }

    println!(
        "{:<25} {:<40} {:<10} Command",
        "Name", "Description", "Version"
    );
    println!("{}", "-".repeat(85));
    for a in &agents {
        let command = if matches!(a.kind, PackageKind::Package) {
            "package"
        } else {
            &a.command
        };
        println!(
            "{:<25} {:<40} {:<10} {}",
            a.id, a.description, a.version, command
        );
    }
    Ok(())
}

fn install(name: &str) -> Result<()> {
    let (publisher, agent_name, version_override) = parse_versioned_id(name)?;
    let agent_id = format!("{publisher}/{agent_name}");

    let index = fetch_index()?;
    let entry = index
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .with_context(|| format!("{agent_id} is not in the store index"))?;

    let installed_version = if matches!(entry.kind, PackageKind::Agent) {
        let agent_toml = fetch_agent_definition(publisher, agent_name, version_override)?;
        let agent_meta =
            toml::from_str::<AgentEntry>(&agent_toml).context("Failed to parse agent definition")?;
        let ver = agent_meta.version.clone();

        let dir = paths::aid_dir().join("agents");
        fs::create_dir_all(&dir)?;
        let target = dir.join(format!("{agent_name}.toml"));
        fs::write(&target, &agent_toml)?;
        println!("Installed {agent_id}@{ver} -> {}", target.display());
        ver
    } else {
        entry.version.clone()
    };

    install_scripts(entry, publisher)?;
    install_skills(entry, publisher)?;
    install_hooks(&entry.hooks)?;

    add_lock_entry(&agent_id, &installed_version)?;
    println!("Hint: run `aid config agents` to see all configured agents.");
    Ok(())
}

fn update(apply: bool) -> Result<()> {
    let entries = read_lockfile()?;
    if entries.is_empty() {
        println!("No store packages are recorded in the lockfile.");
        return Ok(());
    }

    let index = fetch_index()?;
    println!(
        "{:<25} {:<12} {:<12} Status",
        "Name", "Installed", "Available"
    );
    println!("{}", "-".repeat(65));

    let mut updates = Vec::new();
    for entry in &entries {
        let (available_display, status) = if let Some(agent) =
            index.agents.iter().find(|agent| agent.id == entry.id)
        {
            match agent.version.cmp(&entry.version) {
                Ordering::Equal => (agent.version.as_str(), "Up to date"),
                Ordering::Greater => {
                    updates.push(entry.id.clone());
                    (agent.version.as_str(), "Update available")
                }
                Ordering::Less => (agent.version.as_str(), "Installed ahead of index"),
            }
        } else {
            ("-", "Missing from index")
        };

        println!(
            "{:<25} {:<12} {:<12} {}",
            entry.id, entry.version, available_display, status
        );
    }

    if apply {
        if updates.is_empty() {
            println!("No updates to apply.");
        } else {
            println!("Applying {} updates...", updates.len());
            for id in updates {
                install(&id)?;
            }
        }
    }

    Ok(())
}

fn install_scripts(entry: &AgentEntry, publisher: &str) -> Result<()> {
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
            Err(e) => eprintln!("  Warning: script {script}: {e}"),
        }
    }
    Ok(())
}

fn install_skills(entry: &AgentEntry, publisher: &str) -> Result<()> {
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
            Err(e) => eprintln!("  Warning: skill {skill}: {e}"),
        }
    }
    Ok(())
}

fn install_hooks(entries: &[HookEntry]) -> Result<()> {
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
        Ok(content) if !content.trim().is_empty() => toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display())),
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

fn show(name: &str) -> Result<()> {
    let (publisher, agent_name) = parse_id(name)?;
    let url = format!("{REPO_RAW}/agents/{publisher}/{agent_name}.toml");
    let toml = curl_fetch(&url)?;
    print!("{toml}");
    Ok(())
}

fn fetch_index() -> Result<AgentIndex> {
    let body = curl_fetch(&format!("{REPO_RAW}/index.json"))?;
    serde_json::from_str(&body).context("Failed to parse store index")
}

fn parse_id(name: &str) -> Result<(&str, &str)> {
    name.split_once('/')
        .context("Name must be in publisher/name format (e.g. community/nanobanana)")
}

fn parse_versioned_id(name: &str) -> Result<(&str, &str, Option<&str>)> {
    let (publisher, remainder) = parse_id(name)?;
    if let Some((agent_name, version)) = remainder.split_once('@') {
        if version.is_empty() {
            bail!("Version cannot be empty after '@'");
        }
        Ok((publisher, agent_name, Some(version)))
    } else {
        Ok((publisher, remainder, None))
    }
}

fn fetch_agent_definition(
    publisher: &str,
    agent_name: &str,
    version_override: Option<&str>,
) -> Result<String> {
    if let Some(version) = version_override {
        let url = format!("{REPO_RAW}/agents/{publisher}/{agent_name}@{version}.toml");
        match curl_fetch(&url) {
            Ok(content) => return Ok(content),
            Err(err) => eprintln!(
                "Warning: versioned definition {publisher}/{agent_name}@{version} not found ({err}); using latest instead."
            ),
        }
    }

    let url = format!("{REPO_RAW}/agents/{publisher}/{agent_name}.toml");
    curl_fetch(&url)
}

fn curl_fetch(url: &str) -> Result<String> {
    let output = Command::new("curl")
        .args(["-sfL", url])
        .output()
        .context("Failed to run curl")?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        bail!("curl failed (exit {code}) fetching {url}");
    }
    String::from_utf8(output.stdout).context("Response is not valid UTF-8")
}
