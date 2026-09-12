// Builds isolated HOME entries, selecting private directories before symlinking.
// Exports build_isolated_home to the guard; depends on filesystem and Cargo setup.
use anyhow::{Context, Result};
use std::{fs, path::Path};
use super::{DEFAULT_DENYLIST, materialize_claude_dir, remove_isolated_home};

#[path = "home_isolation_cargo.rs"]
mod cargo;

pub(super) fn build_isolated_home(
    real_home: Option<&Path>, isolated_path: &Path, remote_build: bool,
) -> Result<()> {
    let real_home = real_home.context("cannot build isolated HOME: real home directory is unknown")?;
    if !real_home.is_dir() {
        anyhow::bail!("cannot build isolated HOME: '{}' is not a directory", real_home.display());
    }
    if isolated_path.exists() {
        remove_isolated_home(isolated_path, real_home)?;
    }
    fs::create_dir_all(isolated_path).with_context(|| {
        format!("cannot create isolated HOME at '{}'", isolated_path.display())
    })?;
    let entries = fs::read_dir(real_home).with_context(|| {
        format!("cannot read real HOME directory '{}'", real_home.display())
    })?;
    #[cfg(not(unix))]
    anyhow::bail!("HOME isolation requires Unix symlinks");
    for entry in entries {
        let entry = entry.with_context(|| {
            format!("cannot read entry in real HOME '{}'", real_home.display())
        })?;
        build_entry(&entry, isolated_path, remote_build)?;
    }
    if remote_build && !isolated_path.join(".cargo").exists() {
        cargo::materialize(&real_home.join(".cargo"), &isolated_path.join(".cargo"))?;
    }
    Ok(())
}

fn build_entry(entry: &fs::DirEntry, isolated_path: &Path, remote_build: bool) -> Result<()> {
    let file_name = entry.file_name();
    let name_str = file_name.to_string_lossy();
    if DEFAULT_DENYLIST.contains(&name_str.as_ref()) || DEFAULT_DENYLIST.iter().any(|d| {
        name_str.starts_with(&format!("{d}.")) || name_str.starts_with(&format!("{d}-"))
    }) {
        return Ok(());
    }
    let link_dest = isolated_path.join(&file_name);
    let target_path = entry.path();
    if name_str == ".cargo" && remote_build {
        return cargo::materialize(&target_path, &link_dest);
    }
    if name_str == ".claude" && target_path.is_dir() {
        return materialize_claude_dir(&target_path, &link_dest);
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target_path, &link_dest).with_context(|| {
        format!("cannot symlink '{}' -> '{}' in isolated HOME", target_path.display(), link_dest.display())
    })?;
    Ok(())
}
