// Handler for `aid upgrade` CLI command — replaces the aid binary from crates.io.
// Exports: run.
// Deps: crate::paths, crate::store, std::env, std::process.

use anyhow::{bail, Result};
use std::env;
use std::path::PathBuf;
use std::process::Command;

use crate::paths;
use crate::store::Store;

pub fn run(force: bool) -> Result<()> {
    let store = Store::open(&paths::db_path())?;
    let running = store.list_running_tasks()?;
    if !running.is_empty() && !force {
        eprintln!("[aid] {} task(s) still running:", running.len());
        for task in &running {
            eprintln!("  {} — {} ({})", task.id, task.agent_display_name(), task.status.label());
        }
        eprintln!();
        eprintln!("[aid] Use --force to upgrade anyway, or wait for tasks to complete.");
        std::process::exit(1);
    }

    let current = env!("CARGO_PKG_VERSION");
    eprintln!("[aid] Current version: {current}");

    eprintln!("[aid] Installing latest from crates.io...");
    let status = Command::new("cargo").args(["install", "ai-dispatch"]).status()?;
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
    eprintln!("[aid] Upgraded: {current} -> {}", new_version.trim());

    Ok(())
}

#[cfg(target_os = "macos")]
fn home_cargo_bin() -> PathBuf {
    PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".to_string())).join(".cargo/bin")
}
