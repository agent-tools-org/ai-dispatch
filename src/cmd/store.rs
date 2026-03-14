// Handler for `aid store` subcommands — browse, install, show agents from the community store.
// Exports: StoreAction, run_store.
// Deps: serde_json, std::process::Command (curl), crate::paths.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::paths;

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
}

pub enum StoreAction {
    Browse { query: Option<String> },
    Install { name: String },
    Show { name: String },
}

pub fn run_store(action: StoreAction) -> Result<()> {
    match action {
        StoreAction::Browse { query } => browse(query.as_deref()),
        StoreAction::Install { name } => install(&name),
        StoreAction::Show { name } => show(&name),
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
        "{:<25} {:<40} {:<10} {}",
        "Name", "Description", "Version", "Command"
    );
    println!("{}", "-".repeat(85));
    for a in &agents {
        println!(
            "{:<25} {:<40} {:<10} {}",
            a.id, a.description, a.version, a.command
        );
    }
    Ok(())
}

fn install(name: &str) -> Result<()> {
    let (publisher, agent_name) = parse_id(name)?;

    // Install agent TOML
    let url = format!("{REPO_RAW}/agents/{publisher}/{agent_name}.toml");
    let toml = curl_fetch(&url)?;

    let dir = paths::aid_dir().join("agents");
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{agent_name}.toml"));
    fs::write(&target, &toml)?;
    println!("Installed {} -> {}", name, target.display());

    // Install companion scripts (if any)
    let index = fetch_index()?;
    if let Some(entry) = index.agents.iter().find(|a| a.id == name) {
        if !entry.scripts.is_empty() {
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
        }
    }

    println!("Hint: run `aid config agents` to see all configured agents.");
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
