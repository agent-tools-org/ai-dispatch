// Merge-local change custody without using the shared stash stack.
// Exports exact tracked-change capture and untracked-file backup/restore.
// Deps: git plumbing and the standard filesystem APIs.

use std::process::Command;
#[path = "merge_stash_anchor.rs"]
mod anchor;
use anchor::{anchor_tracked_commit, drop_tracked_anchor, sweep_stale_anchors};
#[path = "merge_stash_files.rs"]
mod files;
use files::{
    backup_untracked, ensure_untracked_destinations_free, restore_backup_after_capture_failure,
    restore_untracked, UntrackedBackup,
};
pub(crate) struct LocalChanges {
    tracked_ref: Option<String>,
    tracked_anchor: Option<String>,
    untracked: Option<UntrackedBackup>,
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
    if !has_local_changes(repo_dir)? {
        return Ok(None);
    }
    sweep_stale_anchors(repo_dir)?;
    aid_info!("[aid] Saving local changes before merge...");
    let tracked_ref = create_tracked_stash(repo_dir)?;
    let tracked_anchor = match tracked_ref.as_deref() {
        Some(commit) => Some(anchor_tracked_commit(repo_dir, commit)?),
        None => None,
    };
    after_capture();
    let snapshot = snapshot_ref(repo_dir, tracked_ref.as_deref()).map_err(|error| {
        format_capture_error(
            tracked_ref.as_deref(),
            tracked_anchor.as_deref(),
            &error,
            None,
        )
    })?;
    let untracked = match backup_untracked(repo_dir) {
        Ok(untracked) => untracked,
        Err(error) => {
            return Err(format_capture_error(
                tracked_ref.as_deref(),
                tracked_anchor.as_deref(),
                &error,
                None,
            ));
        }
    };
    // Reset only when both tracked worktree and index still match the captured snapshot.
    if let Err(error) = verify_snapshot(repo_dir, &snapshot, tracked_ref.is_some()) {
        let backup = restore_backup_after_capture_failure(repo_dir, untracked.as_ref());
        return Err(format_capture_error(
            tracked_ref.as_deref(),
            tracked_anchor.as_deref(),
            &error,
            backup,
        ));
    }
    if let Err(error) = clear_worktree(repo_dir) {
        let backup = restore_backup_after_capture_failure(repo_dir, untracked.as_ref());
        return Err(format_capture_error(
            tracked_ref.as_deref(),
            tracked_anchor.as_deref(),
            &error,
            backup,
        ));
    }
    Ok(Some(LocalChanges { tracked_ref, tracked_anchor, untracked }))
}
pub(crate) fn restore_local_changes(
    repo_dir: &str,
    changes: &LocalChanges,
) -> Result<(), String> {
    if let Some(backup) = &changes.untracked {
        ensure_untracked_destinations_free(repo_dir, backup)?;
    }
    if let Some(stash_ref) = &changes.tracked_ref {
        apply_tracked_stash(repo_dir, stash_ref)?;
    }
    if let Some(backup) = &changes.untracked {
        restore_untracked(repo_dir, backup)?;
    }
    if let (Some(anchor), Some(commit)) = (&changes.tracked_anchor, &changes.tracked_ref) {
        drop_tracked_anchor(repo_dir, anchor, commit)?;
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
fn has_local_changes(repo_dir: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "status", "--porcelain"])
        .output()
        .map_err(|error| format!("git status failed: {error}"))?;
    if !output.status.success() {
        return Err(format!("git status failed: {}", first_error_line(&output.stderr)));
    }
    Ok(!output.stdout.is_empty())
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
fn snapshot_ref(repo_dir: &str, tracked_ref: Option<&str>) -> Result<String, String> {
    if let Some(tracked_ref) = tracked_ref {
        return Ok(tracked_ref.to_string());
    }
    let output = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("failed to identify merge snapshot: {error}"))?;
    if !output.status.success() {
        return Err(format!("failed to identify merge snapshot: {}", first_error_line(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn verify_snapshot(repo_dir: &str, snapshot: &str, has_tracked_snapshot: bool) -> Result<(), String> {
    let worktree = Command::new("git")
        .args(["-C", repo_dir, "diff", "--quiet", snapshot, "--"])
        .output()
        .map_err(|error| format!("failed to verify merge snapshot: {error}"))?;
    let index_snapshot = if has_tracked_snapshot {
        let output = Command::new("git")
            .args(["-C", repo_dir, "rev-parse", &format!("{snapshot}^2")])
            .output()
            .map_err(|error| format!("failed to verify merge snapshot: {error}"))?;
        if !output.status.success() {
            return Err("failed to verify merge snapshot".to_string());
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        "HEAD".to_string()
    };
    let index = Command::new("git")
        .args([
            "-C",
            repo_dir,
            "diff",
            "--cached",
            "--quiet",
            &index_snapshot,
            "--",
        ])
        .output()
        .map_err(|error| format!("failed to verify merge snapshot: {error}"))?;
    if worktree.status.code() == Some(1) || index.status.code() == Some(1) {
        return Err("working tree changed after snapshot; reset skipped".to_string());
    }
    if !worktree.status.success() || !index.status.success() {
        return Err("failed to verify merge snapshot".to_string());
    }
    Ok(())
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

fn format_capture_error(
    tracked_ref: Option<&str>,
    tracked_anchor: Option<&str>,
    error: &str,
    untracked: Option<&UntrackedBackup>,
) -> String {
    format!("{error}; recovery handles: {}", recovery_handles(tracked_ref, tracked_anchor, untracked))
}

fn first_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("unknown git error")
        .to_string()
}
