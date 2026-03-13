// Handler for `aid config` — agent configuration and detection.
// Manages agent registry and displays detected AI CLIs.

use anyhow::Result;
use std::sync::Arc;

use crate::agent;
use crate::cli_actions::ConfigAction;
use crate::store::Store;

pub fn run(_store: &Arc<Store>, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Agents => {
            let agents = agent::detect_agents();
            if agents.is_empty() {
                println!("No AI CLI agents detected.");
            } else {
                println!("Detected agents:");
                for a in &agents {
                    println!("  - {}", a.as_str());
                }
            }
        }
        _ => println!("Config not yet implemented"),
    }
    Ok(())
}
