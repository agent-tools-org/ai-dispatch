// CLI handler for `aid broadcast` — send messages to workgroup broadcast file.
// Exports: run.
// Deps: paths, store.

use anyhow::Result;
use chrono::Local;

use crate::store::Store;

pub fn run(store: &Store, group_id: &str, message: &str) -> Result<()> {
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{group_id}' not found"))?;

    let broadcast_path = crate::paths::workspace_dir(group_id).join("broadcast.md");
    let dir = broadcast_path.parent().unwrap();
    std::fs::create_dir_all(dir)?;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&broadcast_path)?;
    let timestamp = Local::now().format("%H:%M:%S");
    writeln!(file, "- [{timestamp}] (orchestrator) {message}")?;

    println!("Broadcast sent to {group_id}");
    Ok(())
}
