// Persistent Codex configuration isolation with shared auth, sessions, and instructions.
// Idempotently mirrors top-level entries under AID_HOME without deleting session paths.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) struct Home {
    pub path: PathBuf,
}

impl Home {
    pub fn create(real: &Path, aid_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(aid_dir)?;
        let real = real.canonicalize().context("Resolve real Codex home")?;
        anyhow::ensure!(
            real.join("sessions").is_dir() && real.join("state_5.sqlite").is_file(),
            "Codex app-server requires existing sessions and state_5.sqlite in the real home"
        );
        let path = aid_dir.join("codex-home");
        std::fs::create_dir_all(&path).context("Create stable Codex home")?;
        anyhow::ensure!(
            std::fs::symlink_metadata(&path)?.file_type().is_dir(),
            "Codex mirror must be a directory, not a symlink"
        );
        anyhow::ensure!(
            path.canonicalize()? != real,
            "Codex mirror must differ from real home"
        );
        let home = Self { path };
        home.refresh(&real)?;
        for entry in std::fs::read_dir(real)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("config.toml") {
                continue;
            }
            #[cfg(unix)]
            match std::os::unix::fs::symlink(entry.path(), home.path.join(&name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    anyhow::ensure!(
                        std::fs::read_link(home.path.join(name))? == entry.path(),
                        "Codex mirror entry points at a different home"
                    );
                }
                Err(error) => return Err(error.into()),
            }
            #[cfg(not(unix))]
            anyhow::bail!("Codex app-server home isolation requires Unix symlinks");
        }
        Ok(home)
    }

    fn refresh(&self, real: &Path) -> Result<()> {
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name();
            anyhow::ensure!(
                !name.to_string_lossy().starts_with("config.toml"),
                "Codex mirror contains excluded user configuration"
            );
            let source_exists = real.join(&name).try_exists()?;
            if entry.file_type()?.is_symlink() {
                if !source_exists {
                    match std::fs::remove_file(entry.path()) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            } else {
                anyhow::ensure!(
                    !source_exists,
                    "Codex mirror local entry collides with real home: {}",
                    name.to_string_lossy()
                );
            }
        }
        Ok(())
    }

    pub fn for_command(cmd: &std::process::Command) -> Result<Self> {
        let real = cmd
            .get_envs()
            .find(|(key, _)| *key == "CODEX_HOME")
            .and_then(|(_, value)| value)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(crate::agent::codex::cli_config::codex_home)?;
        Self::create(&real, &crate::support::paths::aid_dir())
    }
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "home_dispatch_tests.rs"]
mod dispatch_tests;
