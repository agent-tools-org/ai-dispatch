// Handler for `aid upgrade` CLI command — replaces the aid binary from crates.io.
// Exports: run.
// Deps: crate::paths, crate::store, std::env, std::process.

use anyhow::{bail, Result};
use std::process::Command;

use crate::paths;
use crate::store::Store;

pub fn run(force: bool) -> Result<()> {
    let store = Store::open(&paths::db_path())?;
    let running = store.list_running_tasks()?;
    if !running.is_empty() && !force {
        aid_info!("[aid] {} task(s) still running:", running.len());
        for task in &running {
            aid_info!(
                "  {} — {} ({})",
                task.id,
                task.agent_display_name(),
                task.status.label()
            );
        }
        aid_info!("");
        aid_hint!("[aid] Use --force to upgrade anyway, or wait for tasks to complete.");
        std::process::exit(1);
    }

    let current = env!("CARGO_PKG_VERSION");
    aid_info!("[aid] Current version: {current}");

    aid_info!("[aid] Installing latest from crates.io...");
    let status = Command::new("cargo")
        .args(["install", "ai-dispatch"])
        .status()?;
    if !status.success() {
        bail!("cargo install failed");
    }

    #[cfg(target_os = "macos")]
    {
        let aid_path = home_cargo_bin().join("aid");
        if aid_path.exists() {
            let _ = Command::new("codesign")
                .args(["--force", "--sign", "-", &aid_path.display().to_string()])
                .status();
        }
    }

    let output = Command::new("aid").arg("--version").output()?;
    let new_version = String::from_utf8_lossy(&output.stdout);
    aid_info!("[aid] Upgraded: {current} -> {}", new_version.trim());

    Ok(())
}

#[cfg(target_os = "macos")]
fn home_cargo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".cargo/bin")
}
