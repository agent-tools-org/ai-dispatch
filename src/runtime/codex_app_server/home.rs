// Task-scoped Codex configuration isolation with shared auth, sessions, and instructions.
// Mirrors top-level home entries through symlinks and removes only the owned task directory.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) struct Home {
    pub path: PathBuf,
}

impl Home {
    pub fn create(real: &Path, task_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(task_dir)?;
        let real = real.canonicalize().context("Resolve real Codex home")?;
        let path = task_dir.join("codex-home");
        std::fs::create_dir(&path).context("Create task-scoped Codex home")?;
        let guard = Self { path };
        for entry in std::fs::read_dir(real)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == "config.toml" || name.to_string_lossy().starts_with("config.toml.") {
                continue;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(entry.path(), guard.path.join(name))?;
            #[cfg(not(unix))]
            anyhow::bail!("Codex app-server home isolation requires Unix symlinks");
        }
        Ok(guard)
    }

    pub fn for_command(cmd: &std::process::Command, task_dir: &Path) -> Result<Self> {
        let real = cmd
            .get_envs()
            .find(|(key, _)| *key == "CODEX_HOME")
            .and_then(|(_, value)| value)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(crate::agent::codex::cli_config::codex_home)?;
        Self::create(&real, task_dir)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // remove_dir_all does not follow directory symlinks.
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            aid_warn!("[aid] Could not remove task-scoped Codex home: {error}");
        }
    }
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
