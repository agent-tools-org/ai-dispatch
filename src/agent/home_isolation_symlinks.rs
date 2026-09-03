// Symlink leak scanning and atomic replacement for isolated agent homes.
// Exports runtime and doctor repair candidates plus safe replacement helpers.
// Deps: anyhow, std::fs, std::path, and Unix symlink support.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymlinkRepair {
    pub(crate) link_path: PathBuf,
    pub(crate) old_target: PathBuf,
    pub(crate) rewritten_target: PathBuf,
}

pub(crate) struct SymlinkScan {
    pub(crate) repairs: Vec<SymlinkRepair>,
    pub(crate) complete: bool,
}

pub(crate) fn find_rewrites(iso_home: &Path, real_home: &Path) -> SymlinkScan {
    let mut repairs = Vec::new();
    let mut complete = true;
    for bin_dir in operator_bin_dirs(real_home) {
        complete &= collect_rewrites(&bin_dir, iso_home, real_home, &mut repairs);
    }
    SymlinkScan { repairs, complete }
}

pub(crate) fn find_doctor_symlinks(real_home: &Path, aid_dir: &Path) -> Result<Vec<SymlinkRepair>> {
    Ok(scan_doctor_symlinks(real_home, aid_dir).repairs)
}

pub(crate) fn scan_doctor_symlinks(real_home: &Path, aid_dir: &Path) -> SymlinkScan {
    let mut repairs = Vec::new();
    let mut complete = true;
    for bin_dir in operator_bin_dirs(real_home) {
        complete &= collect_doctor_rewrites(&bin_dir, aid_dir, real_home, &mut repairs);
    }
    SymlinkScan { repairs, complete }
}

pub(crate) fn apply_repairs(repairs: &[SymlinkRepair]) -> Result<usize> {
    Ok(apply_repairs_with_status(repairs).repaired)
}

pub(crate) struct RepairSummary {
    pub(crate) repaired: usize,
    pub(crate) complete: bool,
}

pub(crate) fn apply_repairs_with_status(repairs: &[SymlinkRepair]) -> RepairSummary {
    let mut summary = RepairSummary { repaired: 0, complete: true };
    for repair in repairs {
        if !is_repairable(repair) {
            summary.complete = false;
            aid_warn!(
                "[aid] Warning: left symlink '{}' -> '{}': rewritten target '{}' does not exist or is the link itself",
                repair.link_path.display(), repair.old_target.display(), repair.rewritten_target.display()
            );
            continue;
        }
        match replace_symlink(&repair.link_path, &repair.old_target, &repair.rewritten_target) {
            Ok(true) => summary.repaired += 1,
            Ok(false) => {
                summary.complete = false;
                aid_warn!("[aid] Warning: left symlink '{}' because it changed during repair", repair.link_path.display());
            }
            Err(err) => {
                summary.complete = false;
                aid_warn!("[aid] Warning: failed to repair symlink '{}': {err:#}", repair.link_path.display());
            }
        }
    }
    summary
}

pub(crate) fn is_repairable(repair: &SymlinkRepair) -> bool {
    repair.rewritten_target.exists() && repair.rewritten_target != repair.link_path
}

fn operator_bin_dirs(real_home: &Path) -> [PathBuf; 3] {
    [real_home.join(".local/bin"), real_home.join("bin"), real_home.join(".cargo/bin")]
}

fn collect_rewrites(bin_dir: &Path, iso_home: &Path, real_home: &Path, repairs: &mut Vec<SymlinkRepair>) -> bool {
    let (entries, mut complete) = read_symlink_entries(bin_dir);
    for entry in entries {
        let link_path = entry.path();
        let old_target = match fs::read_link(&link_path) {
            Ok(target) => target,
            Err(err) => {
                complete = false;
                aid_warn!("[aid] Warning: cannot read operator symlink '{}': {err}", link_path.display());
                continue;
            }
        };
        let Ok(rest) = old_target.strip_prefix(iso_home) else { continue };
        if rest.as_os_str().is_empty() { continue; }
        if contains_parent_dir(rest) {
            aid_warn!("[aid] Warning: left symlink '{}' -> '{}': rewritten target contains '..'", link_path.display(), old_target.display());
            continue;
        }
        let rewritten_target = real_home.join(rest);
        repairs.push(SymlinkRepair { link_path, old_target, rewritten_target });
    }
    complete
}

fn contains_parent_dir(path: &Path) -> bool {
    path.components().any(|component| component == Component::ParentDir)
}

fn collect_doctor_rewrites(bin_dir: &Path, aid_dir: &Path, real_home: &Path, repairs: &mut Vec<SymlinkRepair>) -> bool {
    let (entries, mut complete) = read_symlink_entries(bin_dir);
    for entry in entries {
        let link_path = entry.path();
        let old_target = match fs::read_link(&link_path) {
            Ok(target) => target,
            Err(err) => {
                complete = false;
                aid_warn!("[aid] Warning: cannot read operator symlink '{}': {err}", link_path.display());
                continue;
            }
        };
        let Some((_iso_home, rest)) = isolated_root_and_rest(&old_target, aid_dir) else { continue };
        repairs.push(SymlinkRepair { link_path, old_target, rewritten_target: real_home.join(rest) });
    }
    complete
}

fn read_symlink_entries(bin_dir: &Path) -> (Vec<fs::DirEntry>, bool) {
    let entries = match fs::read_dir(bin_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), true),
        Err(err) => {
            aid_warn!("[aid] Warning: cannot read operator bin directory '{}': {err}", bin_dir.display());
            return (Vec::new(), false);
        }
    };
    let mut symlinks = Vec::new();
    let mut complete = true;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                complete = false;
                aid_warn!("[aid] Warning: cannot read operator bin entry: {err}");
                continue;
            }
        };
        match entry.file_type() {
            Ok(file_type) if file_type.is_symlink() => symlinks.push(entry),
            Ok(_) => {}
            Err(err) => {
                complete = false;
                aid_warn!("[aid] Warning: cannot inspect operator bin entry '{}': {err}", entry.path().display());
            }
        }
    }
    (symlinks, complete)
}

fn isolated_root_and_rest(target: &Path, aid_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let tasks = aid_dir.join("tasks");
    if let Ok(rest) = target.strip_prefix(&tasks) {
        let mut components = rest.components();
        let task_component = components.next()?;
        if is_dot_component(task_component) { return None; }
        let task_id = task_component.as_os_str().to_owned();
        if components.next()?.as_os_str() != "home" { return None; }
        let after_home = components.as_path();
        if after_home.as_os_str().is_empty() || contains_parent_dir(after_home) { return None; }
        return Some((tasks.join(task_id).join("home"), after_home.to_path_buf()));
    }

    let tmp_home = aid_dir.join("tmp_home");
    let rest = target.strip_prefix(&tmp_home).ok()?;
    let mut components = rest.components();
    let isolated_component = components.next()?;
    if is_dot_component(isolated_component) { return None; }
    let isolated_dir = tmp_home.join(isolated_component.as_os_str());
    let after_isolated_name = components.as_path();
    let after_home = after_isolated_name.strip_prefix("home").ok()?;
    if after_home.as_os_str().is_empty() || contains_parent_dir(after_home) { return None; }
    Some((isolated_dir.join("home"), after_home.to_path_buf()))
}

fn is_dot_component(component: Component<'_>) -> bool {
    matches!(component, Component::CurDir | Component::ParentDir)
}

pub(crate) fn replace_symlink(link_path: &Path, old_target: &Path, target: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        let parent = link_path.parent().context("symlink has no parent directory")?;
        let temp_path = create_temp_symlink(parent, target)?;
        let still_matches = match link_still_matches(link_path, old_target) {
            Ok(still_matches) => still_matches,
            Err(err) => {
                let _ = fs::remove_file(&temp_path);
                return Err(err);
            }
        };
        if !still_matches {
            let _ = fs::remove_file(&temp_path);
            return Ok(false);
        }
        if let Err(err) = fs::rename(&temp_path, link_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(err).with_context(|| format!("cannot atomically replace symlink '{}'", link_path.display()));
        }
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = (link_path, old_target, target);
        anyhow::bail!("isolated HOME symlink repair requires Unix")
    }
}

#[cfg(unix)]
fn create_temp_symlink(parent: &Path, target: &Path) -> Result<PathBuf> {
    loop {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(".aid-symlink-repair-{}-{sequence}", std::process::id()));
        match std::os::unix::fs::symlink(target, &temp_path) {
            Ok(()) => return Ok(temp_path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err).with_context(|| format!("cannot create temporary symlink '{}' -> '{}'", temp_path.display(), target.display())),
        }
    }
}

#[cfg(unix)]
fn link_still_matches(link_path: &Path, old_target: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(link_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("cannot stat symlink '{}'", link_path.display())),
    };
    if !metadata.file_type().is_symlink() { return Ok(false); }
    let current_target = match fs::read_link(link_path) {
        Ok(target) => target,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("cannot re-read symlink '{}'", link_path.display())),
    };
    Ok(current_target == old_target)
}
