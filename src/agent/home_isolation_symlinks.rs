// Symlink leak scanning and atomic replacement for isolated agent homes.
// Exports runtime and doctor repair candidates plus safe replacement helpers.
// Deps: anyhow, std::fs, std::path, and Unix symlink support.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymlinkRepair {
    pub(crate) link_path: PathBuf,
    pub(crate) old_target: PathBuf,
    pub(crate) rewritten_target: PathBuf,
}

pub(crate) fn find_rewrites(iso_home: &Path, real_home: &Path) -> Result<Vec<SymlinkRepair>> {
    let mut repairs = Vec::new();
    for bin_dir in operator_bin_dirs(real_home) {
        collect_rewrites(&bin_dir, iso_home, real_home, &mut repairs)?;
    }
    Ok(repairs)
}

pub(crate) fn find_doctor_symlinks(real_home: &Path, aid_dir: &Path) -> Result<Vec<SymlinkRepair>> {
    let mut repairs = Vec::new();
    for bin_dir in operator_bin_dirs(real_home) {
        collect_doctor_rewrites(&bin_dir, aid_dir, real_home, &mut repairs)?;
    }
    Ok(repairs)
}

pub(crate) fn apply_repairs(repairs: &[SymlinkRepair]) -> Result<usize> {
    let mut repaired = 0;
    for repair in repairs {
        if !is_repairable(repair) {
            aid_warn!(
                "[aid] Warning: left symlink '{}' -> '{}': rewritten target '{}' does not exist or is the link itself",
                repair.link_path.display(),
                repair.old_target.display(),
                repair.rewritten_target.display()
            );
            continue;
        }
        replace_symlink(&repair.link_path, &repair.rewritten_target)?;
        repaired += 1;
    }
    Ok(repaired)
}

pub(crate) fn is_repairable(repair: &SymlinkRepair) -> bool {
    repair.rewritten_target.exists() && repair.rewritten_target != repair.link_path
}

fn operator_bin_dirs(real_home: &Path) -> [PathBuf; 3] {
    [
        real_home.join(".local/bin"),
        real_home.join("bin"),
        real_home.join(".cargo/bin"),
    ]
}

fn collect_rewrites(
    bin_dir: &Path,
    iso_home: &Path,
    real_home: &Path,
    repairs: &mut Vec<SymlinkRepair>,
) -> Result<()> {
    for entry in read_symlink_entries(bin_dir)? {
        let link_path = entry.path();
        let old_target = fs::read_link(&link_path)
            .with_context(|| format!("cannot read symlink '{}'", link_path.display()))?;
        let Ok(rest) = old_target.strip_prefix(iso_home) else {
            continue;
        };
        let rewritten_target = real_home.join(rest);
        repairs.push(SymlinkRepair {
            link_path,
            old_target,
            rewritten_target,
        });
    }
    Ok(())
}

fn collect_doctor_rewrites(
    bin_dir: &Path,
    aid_dir: &Path,
    real_home: &Path,
    repairs: &mut Vec<SymlinkRepair>,
) -> Result<()> {
    for entry in read_symlink_entries(bin_dir)? {
        let link_path = entry.path();
        let old_target = fs::read_link(&link_path)
            .with_context(|| format!("cannot read symlink '{}'", link_path.display()))?;
        let Some((_iso_home, rest)) = isolated_root_and_rest(&old_target, aid_dir) else {
            continue;
        };
        repairs.push(SymlinkRepair {
            link_path,
            old_target,
            rewritten_target: real_home.join(rest),
        });
    }
    Ok(())
}

fn read_symlink_entries(bin_dir: &Path) -> Result<Vec<fs::DirEntry>> {
    if !bin_dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(bin_dir)
        .with_context(|| format!("cannot read operator bin directory '{}'", bin_dir.display()))?;
    let mut symlinks = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            symlinks.push(entry);
        }
    }
    Ok(symlinks)
}

fn isolated_root_and_rest(target: &Path, aid_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let tasks = aid_dir.join("tasks");
    if let Ok(rest) = target.strip_prefix(&tasks) {
        let mut components = rest.components();
        let task_id = components.next()?.as_os_str().to_owned();
        if components.next()?.as_os_str() != "home" {
            return None;
        }
        return Some((tasks.join(task_id).join("home"), components.as_path().to_path_buf()));
    }

    let tmp_home = aid_dir.join("tmp_home");
    let rest = target.strip_prefix(&tmp_home).ok()?;
    let mut components = rest.components();
    let isolated_name = components.next()?.as_os_str().to_owned();
    let after_isolated_name = components.as_path().to_path_buf();
    let isolated_dir = tmp_home.join(isolated_name);
    if let Ok(after_home) = after_isolated_name.strip_prefix("home") {
        return Some((isolated_dir.join("home"), after_home.to_path_buf()));
    }
    Some((isolated_dir, after_isolated_name))
}

pub(crate) fn replace_symlink(link_path: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let parent = link_path
            .parent()
            .context("symlink has no parent directory")?;
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".aid-symlink-repair-{}-{sequence}",
            std::process::id()
        ));
        std::os::unix::fs::symlink(target, &temp_path).with_context(|| {
            format!(
                "cannot create temporary symlink '{}' -> '{}'",
                temp_path.display(),
                target.display()
            )
        })?;
        if let Err(err) = fs::rename(&temp_path, link_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(err).with_context(|| {
                format!("cannot atomically replace symlink '{}'", link_path.display())
            });
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (link_path, target);
        anyhow::bail!("isolated HOME symlink repair requires Unix")
    }
}
