// Handler for `aid store` subcommands — browse, install, show, update agents from the community store.
// Exports: run_store.
// Deps: serde, serde_json, toml, std::fs, std::process::Command (curl), crate::support::paths.

mod install_assets;
use install_assets::{install_hooks, install_scripts, install_skills};

use crate::cli::StoreCommands;
use crate::cmd::store_lock::{add_lock_entry, read_lockfile};
use crate::support::paths;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::cmp::Ordering;
use std::fs;
use std::process::Command;

const REPO_RAW: &str = "https://raw.githubusercontent.com/agent-tools-org/aid-agents/main";

#[derive(Deserialize)]
struct AgentIndex {
    agents: Vec<AgentEntry>,
}

#[derive(Deserialize)]
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

pub fn run_store(action: StoreCommands) -> Result<()> {
    match action {
        StoreCommands::Browse { query } => browse(query.as_deref()),
        StoreCommands::Install { name } => install(&name),
        StoreCommands::Show { name } => show(&name),
        StoreCommands::Update { apply } => update(apply),
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

    println!("{:<25} {:<40} {:<10} Command", "Name", "Description", "Version");
    println!("{}", "-".repeat(85));
    for a in &agents {
        let command = if matches!(a.kind, PackageKind::Package) {
            "package"
        } else {
            &a.command
        };
        println!("{:<25} {:<40} {:<10} {}", a.id, a.description, a.version, command);
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
    println!("{:<25} {:<12} {:<12} Status", "Name", "Installed", "Available");
    println!("{}", "-".repeat(65));

    let mut updates = Vec::new();
    for entry in &entries {
        let (available_display, status) =
            if let Some(agent) = index.agents.iter().find(|agent| agent.id == entry.id) {
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
            Err(err) => aid_warn!(
                "Warning: versioned definition {publisher}/{agent_name}@{version} not found \
                ({err}); using latest instead."
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
