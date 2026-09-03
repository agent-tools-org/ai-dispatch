// Isolated HOME directory per task dispatch to prevent orchestrator identity leaks.
// Exports: IsolatedHomeGuard, DEFAULT_DENYLIST, resolve_real_home, reconciliation.
// Deps: std::fs, std::path::{Path, PathBuf}, libc (unix passwd fallback).

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

#[path = "home_isolation_symlinks.rs"]
mod symlinks;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub const DEFAULT_DENYLIST: &[&str] = &[
    ".anthropic",
    ".agents",
    ".agent",
];

pub(crate) use symlinks::{apply_repairs, find_doctor_symlinks, SymlinkRepair};

pub struct IsolatedHomeGuard {
    path: PathBuf,
    real_home: PathBuf,
}

pub fn resolve_real_home() -> anyhow::Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if home.is_dir() {
            return Ok(home);
        }
        anyhow::bail!("HOME is set to '{}' but is not a directory", home.display());
    }
    passwd_home()
}

pub fn reconcile_leaked_symlinks(iso_home: &Path, real_home: &Path) -> anyhow::Result<()> {
    for repair in symlinks::find_rewrites(iso_home, real_home)? {
        if repair.rewritten_target.exists() {
            symlinks::replace_symlink(&repair.link_path, &repair.rewritten_target)?;
        } else {
            aid_warn!(
                "[aid] Warning: left symlink '{}' -> '{}': rewritten target '{}' does not exist",
                repair.link_path.display(),
                repair.old_target.display(),
                repair.rewritten_target.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn remove_isolated_home(path: &Path, real_home: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if let Err(err) = reconcile_leaked_symlinks(path, real_home) {
        aid_warn!(
            "[aid] Warning: failed to reconcile symlinks before removing '{}': {err:#}",
            path.display()
        );
    }
    fs::remove_dir_all(path)
        .with_context(|| format!("cannot remove isolated HOME at '{}'", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn passwd_home() -> anyhow::Result<PathBuf> {
    unsafe {
        let uid = libc::getuid();
        let entry = libc::getpwuid(uid);
        if entry.is_null() {
            anyhow::bail!("HOME is unset and passwd lookup failed for uid {uid}");
        }
        let dir = std::ffi::CStr::from_ptr((*entry).pw_dir);
        let home = PathBuf::from(OsStr::from_bytes(dir.to_bytes()));
        if !home.is_dir() {
            anyhow::bail!(
                "HOME is unset; passwd home '{}' is not a directory",
                home.display()
            );
        }
        Ok(home)
    }
}

#[cfg(not(unix))]
fn passwd_home() -> anyhow::Result<PathBuf> {
    anyhow::bail!("HOME is unset and passwd home resolution is unavailable on this platform")
}

impl IsolatedHomeGuard {
    pub fn create(task_id: Option<&str>) -> anyhow::Result<Self> {
        let real_home = resolve_real_home()?;
        Self::create_from_home(Some(real_home.as_path()), task_id)
    }

    pub fn create_from_home(real_home: Option<&Path>, task_id: Option<&str>) -> anyhow::Result<Self> {
        let base_dir = match task_id {
            Some(id) => crate::paths::task_dir(id),
            None => {
                let pid = std::process::id();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                crate::paths::aid_dir()
                    .join("tmp_home")
                    .join(format!("iso-{pid}-{now}"))
            }
        };
        let target_home = base_dir.join("home");
        let real_home_path = real_home.map(Path::to_path_buf);
        Self::build_isolated_home(real_home, &target_home)?;
        let Some(real_home) = real_home_path else {
            anyhow::bail!("cannot build isolated HOME: real home directory is unknown");
        };
        Ok(Self {
            path: target_home,
            real_home,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn apply_toolchain_env(&self, cmd: &mut Command) {
        cmd.env("CARGO_HOME", self.real_home.join(".cargo"));
        cmd.env("RUSTUP_HOME", self.real_home.join(".rustup"));
    }

    fn build_isolated_home(real_home: Option<&Path>, isolated_path: &Path) -> anyhow::Result<()> {
        let Some(real_home) = real_home else {
            anyhow::bail!("cannot build isolated HOME: real home directory is unknown");
        };
        if !real_home.is_dir() {
            anyhow::bail!(
                "cannot build isolated HOME: '{}' is not a directory",
                real_home.display()
            );
        }

        if isolated_path.exists() {
            remove_isolated_home(isolated_path, real_home)?;
        }
        fs::create_dir_all(isolated_path).with_context(|| {
            format!(
                "cannot create isolated HOME at '{}'",
                isolated_path.display()
            )
        })?;

        let entries = fs::read_dir(real_home).with_context(|| {
            format!("cannot read real HOME directory '{}'", real_home.display())
        })?;

        #[cfg(not(unix))]
        {
            anyhow::bail!("HOME isolation requires Unix symlinks");
        }

        for entry in entries {
            let entry = entry.with_context(|| {
                format!("cannot read entry in real HOME '{}'", real_home.display())
            })?;
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if DEFAULT_DENYLIST.contains(&name_str.as_ref())
                || DEFAULT_DENYLIST
                    .iter()
                    .any(|d| name_str.starts_with(&format!("{d}.")) || name_str.starts_with(&format!("{d}-")))
            {
                continue;
            }
            let link_dest = isolated_path.join(&file_name);
            let target_path = entry.path();

            if name_str == ".claude" && target_path.is_dir() {
                materialize_claude_dir(&target_path, &link_dest)?;
                continue;
            }

            #[cfg(unix)]
            std::os::unix::fs::symlink(&target_path, &link_dest).with_context(|| {
                format!(
                    "cannot symlink '{}' -> '{}' in isolated HOME",
                    target_path.display(),
                    link_dest.display()
                )
            })?;
        }
        Ok(())
    }
}

fn is_claude_instruction_entry(name: &str) -> bool {
    const INSTRUCTION_PREFIXES: &[&str] = &[
        "CLAUDE.md",
        "settings.json",
        "settings.local.json",
        ".mcp.json",
        "skills",
        "agents",
        "commands",
        "plugins",
        "hooks",
        "tools",
        "rules",
        "workflows",
        "agent-memory",
        "memory",
        "plans",
        "config-sync",
        "config-sync-repo",
    ];

    INSTRUCTION_PREFIXES.iter().any(|prefix| {
        name == *prefix
            || name.starts_with(&format!("{prefix}."))
            || name.starts_with(&format!("{prefix}-"))
    })
}

fn materialize_claude_dir(real_claude_dir: &Path, isolated_claude_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(isolated_claude_dir).with_context(|| {
        format!(
            "cannot create isolated .claude directory at '{}'",
            isolated_claude_dir.display()
        )
    })?;

    let entries = fs::read_dir(real_claude_dir).with_context(|| {
        format!("cannot read real .claude directory '{}'", real_claude_dir.display())
    })?;

    for entry in entries {
        let entry = entry.with_context(|| {
            format!("cannot read entry in real .claude '{}'", real_claude_dir.display())
        })?;
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if is_claude_instruction_entry(name_str.as_ref()) {
            continue;
        }

        let link_dest = isolated_claude_dir.join(&file_name);
        let target_path = entry.path();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_path, &link_dest).with_context(|| {
            format!(
                "cannot symlink '{}' -> '{}' in isolated .claude",
                target_path.display(),
                link_dest.display()
            )
        })?;
    }
    Ok(())
}

impl Drop for IsolatedHomeGuard {
    fn drop(&mut self) {
        if let Err(err) = remove_isolated_home(&self.path, &self.real_home) {
            aid_warn!(
                "[aid] Warning: failed to remove isolated HOME '{}': {err:#}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
#[path = "home_isolation_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "home_isolation_symlink_tests.rs"]
mod symlink_tests;
