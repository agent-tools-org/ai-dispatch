// Helpers for tracking installed store packages via lockfile entries.
// Exports: read_lockfile, add_lock_entry.
// Deps: anyhow, chrono, serde, toml, std::fs, crate::paths.

use anyhow::{Context, Result};
use chrono::Local;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

use crate::paths;

#[derive(Deserialize, Clone)]
pub(crate) struct LockEntry {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) installed_at: String,
}

#[derive(Deserialize, Default)]
struct StoreLock {
    #[serde(default)]
    package: Vec<LockEntry>,
}

fn lockfile_path() -> PathBuf {
    paths::aid_dir().join("store.lock")
}

pub(crate) fn read_lockfile() -> Result<Vec<LockEntry>> {
    let path = lockfile_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let body = fs::read_to_string(&path)?;
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }

    let lock: StoreLock =
        toml::from_str(&body).context("Failed to parse store lockfile")?;
    Ok(lock.package)
}

fn write_lockfile(entries: &[LockEntry]) -> Result<()> {
    let path = lockfile_path();
    fs::create_dir_all(paths::aid_dir())?;
    let mut serialized = String::new();
    for entry in entries {
        serialized.push_str("[[package]]\n");
        serialized.push_str("id = \"");
        serialized.push_str(&entry.id);
        serialized.push_str("\"\n");
        serialized.push_str("version = \"");
        serialized.push_str(&entry.version);
        serialized.push_str("\"\n");
        serialized.push_str("installed_at = \"");
        serialized.push_str(&entry.installed_at);
        serialized.push_str("\"\n\n");
    }
    fs::write(path, serialized)?;
    Ok(())
}

pub(crate) fn add_lock_entry(id: &str, version: &str) -> Result<()> {
    let mut entries = read_lockfile()?;
    let installed_at = Local::now().to_rfc3339();

    if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
        entry.version = version.to_string();
        entry.installed_at = installed_at;
    } else {
        entries.push(LockEntry {
            id: id.to_string(),
            version: version.to_string(),
            installed_at,
        });
    }

    write_lockfile(&entries)?;
    Ok(())
}
