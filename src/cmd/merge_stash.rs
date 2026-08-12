// Merge-local change custody without using the shared stash stack.
// Exports exact tracked-change capture and untracked-file backup/restore.
// Deps: git plumbing and the standard filesystem APIs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
pub(crate) struct LocalChanges {
    tracked_ref: Option<String>,
    tracked_anchor: Option<String>,
    untracked: Option<UntrackedBackup>,
}

struct UntrackedBackup {
    root: PathBuf,
    paths: Vec<PathBuf>,
}
pub(crate) fn stash_local_changes(repo_dir: &str) -> Result<Option<LocalChanges>, String> {
    capture_local_changes(repo_dir, || {})
}
#[cfg(test)]
pub(crate) fn stash_local_changes_with_hook<F>(
    repo_dir: &str,
    after_capture: F,
) -> Result<Option<LocalChanges>, String>
where
    F: FnOnce(),
{
    capture_local_changes(repo_dir, after_capture)
}
fn capture_local_changes<F>(repo_dir: &str, after_capture: F) -> Result<Option<LocalChanges>, String>
where
    F: FnOnce(),
{
    if !has_local_changes(repo_dir) {
        return Ok(None);
    }
    aid_info!("[aid] Saving local changes before merge...");
    let tracked_ref = create_tracked_stash(repo_dir)?;
    let tracked_anchor = match tracked_ref.as_deref() {
        Some(commit) => Some(anchor_tracked_commit(repo_dir, commit)?),
        None => None,
    };
    after_capture();
    let untracked = match backup_untracked(repo_dir) {
        Ok(untracked) => untracked,
        Err(error) => {
            return Err(format_capture_error(
                tracked_ref.as_deref(),
                tracked_anchor.as_deref(),
                &error,
            ));
        }
    };
    if let Err(error) = clear_worktree(repo_dir) {
        if let Some(backup) = &untracked {
            let _ = restore_untracked(repo_dir, backup);
        }
        return Err(format_capture_error(
            tracked_ref.as_deref(),
            tracked_anchor.as_deref(),
            &error,
        ));
    }
    Ok(Some(LocalChanges { tracked_ref, tracked_anchor, untracked }))
}
pub(crate) fn restore_local_changes(
    repo_dir: &str,
    changes: &LocalChanges,
) -> Result<(), String> {
    if let Some(stash_ref) = &changes.tracked_ref {
        apply_tracked_stash(repo_dir, stash_ref)?;
    }
    if let (Some(anchor), Some(commit)) = (&changes.tracked_anchor, &changes.tracked_ref) {
        drop_tracked_anchor(repo_dir, anchor, commit)?;
    }
    if let Some(backup) = &changes.untracked {
        restore_untracked(repo_dir, backup)?;
    }
    Ok(())
}
pub(crate) fn restore_untracked_after_failed_merge(
    repo_dir: &str,
    changes: &LocalChanges,
) -> Result<(), String> {
    if let Some(backup) = &changes.untracked {
        restore_untracked(repo_dir, backup)?;
    }
    Ok(())
}
pub(crate) fn format_stash_restore_error(changes: &LocalChanges, error: &str) -> String {
    format!(
        "failed to restore merge-local changes ({}): {error}; recover manually before retrying",
        recovery_handles(
            changes.tracked_ref.as_deref(),
            changes.tracked_anchor.as_deref(),
            changes.untracked.as_ref(),
        )
    )
}
fn recovery_handles(
    tracked_ref: Option<&str>,
    tracked_anchor: Option<&str>,
    untracked: Option<&UntrackedBackup>,
) -> String {
    let handles = [
        tracked_ref.map(|value| format!("tracked commit {value}")),
        tracked_anchor.map(|value| format!("reachable ref {value}")),
        untracked.map(|backup| format!("untracked backup {}", backup.root.display())),
    ];
    let handles: Vec<_> = handles.into_iter().flatten().collect();
    if handles.is_empty() {
        "local-change backup".to_string()
    } else {
        handles.join(", ")
    }
}
fn has_local_changes(repo_dir: &str) -> bool {
    Command::new("git")
        .args(["-C", repo_dir, "status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false)
}
fn create_tracked_stash(repo_dir: &str) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "create"])
        .output()
        .map_err(|error| format!("failed to capture tracked changes: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to capture tracked changes: {}",
            first_error_line(&output.stderr)
        ));
    }
    let reference = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!reference.is_empty()).then_some(reference))
}
fn anchor_tracked_commit(repo_dir: &str, commit: &str) -> Result<String, String> {
    let anchor = format!("refs/aid/merge-local/{commit}");
    let output = Command::new("git")
        .args(["-C", repo_dir, "update-ref", &anchor, commit])
        .output()
        .map_err(|error| format!("failed to retain tracked changes {commit}: {error}"))?;
    if output.status.success() {
        Ok(anchor)
    } else {
        Err(format!("failed to retain tracked changes {commit}: {}", first_error_line(&output.stderr)))
    }
}
fn backup_untracked(repo_dir: &str) -> Result<Option<UntrackedBackup>, String> {
    let paths = untracked_paths(repo_dir)?;
    if paths.is_empty() {
        return Ok(None);
    }
    let root = unique_backup_root()?;
    for relative in &paths {
        let source = Path::new(repo_dir).join(relative);
        let destination = root.join(relative);
        if let Err(error) = copy_entry(&source, &destination) {
            let _ = fs::remove_dir_all(&root);
            return Err(format!("failed to back up untracked file {}: {error}", relative.display()));
        }
    }
    let mut removed = Vec::new();
    for relative in &paths {
        let source = Path::new(repo_dir).join(relative);
        if let Err(error) = fs::remove_file(&source) {
            for restored in &removed {
                let _ = copy_entry(&root.join(restored), &Path::new(repo_dir).join(restored));
            }
            let _ = fs::remove_dir_all(&root);
            return Err(format!("failed to clear untracked file {}: {error}", relative.display()));
        }
        removed.push(relative.clone());
    }
    Ok(Some(UntrackedBackup { root, paths }))
}
fn untracked_paths(repo_dir: &str) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "ls-files", "--others", "--exclude-standard", "-z"])
        .output()
        .map_err(|error| format!("failed to list untracked files: {error}"))?;
    if !output.status.success() {
        return Err(format!("failed to list untracked files: {}", first_error_line(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
}
fn unique_backup_root() -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to create local-change backup name: {error}"))?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("aid-merge-local-{}-{timestamp}", std::process::id()));
    fs::create_dir(&root).map_err(|error| format!("failed to create local-change backup: {error}"))?;
    Ok(root)
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file_type = fs::symlink_metadata(source)
        .map_err(|error| error.to_string())?
        .file_type();
    if file_type.is_symlink() {
        return copy_symlink(source, destination);
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(fs::read_link(source).map_err(|error| error.to_string())?, destination)
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), String> {
    let target = fs::read_link(source).map_err(|error| error.to_string())?;
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
    .map_err(|error| error.to_string())
}

fn clear_worktree(repo_dir: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "reset", "--hard", "HEAD"])
        .output()
        .map_err(|error| format!("failed to clear tracked changes: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("failed to clear tracked changes: {}", first_error_line(&output.stderr)))
    }
}

fn restore_untracked(repo_dir: &str, backup: &UntrackedBackup) -> Result<(), String> {
    for relative in &backup.paths {
        let destination = Path::new(repo_dir).join(relative);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(format!("untracked path already exists: {}", relative.display()));
        }
        copy_entry(&backup.root.join(relative), &destination)?;
    }
    fs::remove_dir_all(&backup.root)
        .map_err(|error| format!("failed to remove local-change backup: {error}"))
}

fn apply_tracked_stash(repo_dir: &str, stash_ref: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "apply", "--index", stash_ref])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_error_line(&output.stderr))
    }
}

fn drop_tracked_anchor(repo_dir: &str, anchor: &str, commit: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "update-ref", "-d", anchor, commit])
        .output()
        .map_err(|error| format!("failed to drop tracked-change ref {anchor}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("failed to drop tracked-change ref {anchor}: {}", first_error_line(&output.stderr)))
    }
}

fn format_capture_error(
    tracked_ref: Option<&str>,
    tracked_anchor: Option<&str>,
    error: &str,
) -> String {
    format!("{error}; recovery handles: {}", recovery_handles(tracked_ref, tracked_anchor, None))
}

fn first_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("unknown git error")
        .to_string()
}
