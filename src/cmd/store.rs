// Handler for `aid store` subcommands — browse, install, show agents from the community store.
// Exports: StoreAction, run_store.
// Deps: serde_json, std::process::Command (curl), crate::paths.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::process::Command;

use crate::paths;

const INDEX_URL: &str =
    "https://raw.githubusercontent.com/sunoj/aid-agents/main/index.json";
const AGENTS_BASE: &str =
    "https://raw.githubusercontent.com/sunoj/aid-agents/main/agents";

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
}

pub enum StoreAction {
    Browse,
    Install { name: String },
    Show { name: String },
}

pub fn run_store(action: StoreAction) -> Result<()> {
    match action {
        StoreAction::Browse => browse(),
        StoreAction::Install { name } => install(&name),
        StoreAction::Show { name } => show(&name),
    }
}

fn browse() -> Result<()> {
    let body = curl_fetch(INDEX_URL)?;
    let index: AgentIndex =
        serde_json::from_str(&body).context("Failed to parse store index")?;

    if index.agents.is_empty() {
        println!("Store is empty.");
        return Ok(());
    }

    println!(
        "{:<25} {:<40} {:<10} {}",
        "Name", "Description", "Version", "Command"
    );
    println!("{}", "-".repeat(85));
    for a in &index.agents {
        println!(
            "{:<25} {:<40} {:<10} {}",
            a.id, a.description, a.version, a.command
        );
    }
    Ok(())
}

fn install(name: &str) -> Result<()> {
    let (publisher, agent_name) = parse_id(name)?;
    let url = format!("{AGENTS_BASE}/{publisher}/{agent_name}.toml");
    let toml = curl_fetch(&url)?;

    let dir = paths::aid_dir().join("agents");
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{agent_name}.toml"));

    fs::write(&target, &toml)?;
    println!("Installed {} -> {}", name, target.display());
    println!("Hint: run `aid config agents` to see all configured agents.");
    Ok(())
}

fn show(name: &str) -> Result<()> {
    let (publisher, agent_name) = parse_id(name)?;
    let url = format!("{AGENTS_BASE}/{publisher}/{agent_name}.toml");
    let toml = curl_fetch(&url)?;
    print!("{toml}");
    Ok(())
}

fn parse_id(name: &str) -> Result<(&str, &str)> {
    name.split_once('/')
        .context("Name must be in publisher/name format (e.g. sunoj/aider)")
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
