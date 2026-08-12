// Captures merge-local changes in Git's durable stash list.
// Exports exact stash capture, restoration, and conflict recovery helpers.
// Deps: Git stash plumbing and standard filesystem path inspection.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct LocalChanges {
    stash_ref: String,
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
    let message = unique_stash_message()?;
    aid_info!("[aid] Saving local changes before merge...");
    push_stash(repo_dir, &message)?;
    let stash_ref = find_stash(repo_dir, &message).map_err(|error| {
        format_capture_error(None, &message, &error)
    })?;
    // Git captures and clears these paths in one operation; later edits are never reset by aid.
    after_capture();
    Ok(Some(LocalChanges { stash_ref }))
}

pub(crate) fn restore_local_changes(
    repo_dir: &str,
    changes: &LocalChanges,
) -> Result<(), String> {
    ensure_stash_untracked_paths_free(repo_dir, &changes.stash_ref)?;
    apply_stash(repo_dir, &changes.stash_ref, true)
}

pub(crate) fn restore_untracked_after_failed_merge(
    repo_dir: &str,
    changes: &LocalChanges,
) -> Result<(), String> {
    let Some(untracked_tree) = stash_untracked_tree(repo_dir, &changes.stash_ref)? else {
        return Ok(());
    };
    let paths = tree_paths(repo_dir, &untracked_tree)?;
    ensure_paths_free(repo_dir, &changes.stash_ref, &paths)?;
    if paths.is_empty() {
        return Ok(());
    }
    let source = format!("{untracked_tree}^{{tree}}");
    let mut args = vec![
        "-C",
        repo_dir,
        "restore",
        "--source",
        &source,
        "--worktree",
        "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| format!("failed to restore untracked merge-local changes: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to restore untracked merge-local changes: {}",
            first_error_line(&output.stderr)
        ))
    }
}

pub(crate) fn format_stash_restore_error(changes: &LocalChanges, error: &str) -> String {
    format!(
        "failed to restore merge-local changes (stash commit {} visible in git stash list): {error}; recover manually before retrying",
        changes.stash_ref
    )
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

fn unique_stash_message() -> Result<String, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to create merge stash identity: {error}"))?
        .as_nanos();
    Ok(format!(
        "aid merge-local {}-{timestamp}",
        std::process::id()
    ))
}

fn push_stash(repo_dir: &str, message: &str) -> Result<(), String> {
    // `stash create` cannot include untracked files; push -u stores both kinds durably at once.
    let output = Command::new("git")
        .args([
            "-C",
            repo_dir,
            "stash",
            "push",
            "--include-untracked",
            "--quiet",
            "--message",
            message,
        ])
        .output()
        .map_err(|error| format!("failed to capture merge-local changes: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to capture merge-local changes: {}",
            first_error_line(&output.stderr)
        ))
    }
}

fn find_stash(repo_dir: &str, message: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "list", "--format=%H%x09%gs"])
        .output()
        .map_err(|error| format!("failed to identify merge-local stash: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to identify merge-local stash: {}",
            first_error_line(&output.stderr)
        ));
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((commit, subject)) = line.split_once('\t') else {
            continue;
        };
        if subject.contains(message) {
            return Ok(commit.to_string());
        }
    }
    Err(format!("stash message {message} was not found in git stash list"))
}

fn apply_stash(repo_dir: &str, stash_ref: &str, with_index: bool) -> Result<(), String> {
    let mut args = vec!["-C", repo_dir, "stash", "apply"];
    if with_index {
        args.push("--index");
    }
    args.push(stash_ref);
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| format!("failed to apply merge-local stash {stash_ref}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to apply merge-local stash {stash_ref}: {}",
            first_error_line(&output.stderr)
        ))
    }
}

fn stash_untracked_tree(repo_dir: &str, stash_ref: &str) -> Result<Option<String>, String> {
    let parent = format!("{stash_ref}^3");
    let output = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", &parent])
        .output()
        .map_err(|error| format!("failed to inspect merge-local stash {stash_ref}: {error}"))?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()))
    } else {
        Ok(None)
    }
}

fn ensure_stash_untracked_paths_free(repo_dir: &str, stash_ref: &str) -> Result<(), String> {
    let Some(tree) = stash_untracked_tree(repo_dir, stash_ref)? else {
        return Ok(());
    };
    let paths = tree_paths(repo_dir, &tree)?;
    ensure_paths_free(repo_dir, stash_ref, &paths)
}

fn tree_paths(repo_dir: &str, tree: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "ls-tree", "-r", "--name-only", "-z", tree])
        .output()
        .map_err(|error| format!("failed to inspect merge-local stash: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to inspect merge-local stash: {}",
            first_error_line(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

fn ensure_paths_free(repo_dir: &str, stash_ref: &str, paths: &[String]) -> Result<(), String> {
    let collisions: Vec<_> = paths
        .iter()
        .filter(|path| path_has_existing_parent(repo_dir, path))
        .collect();
    if collisions.is_empty() {
        return Ok(());
    }
    Err(format!(
        "untracked files left in stash commit {stash_ref} (visible in git stash list): {}",
        collisions.iter().map(|path| path.as_str()).collect::<Vec<_>>().join(", ")
    ))
}

fn path_has_existing_parent(repo_dir: &str, relative: &str) -> bool {
    let path = Path::new(repo_dir).join(relative);
    if fs::symlink_metadata(&path).is_ok() {
        return true;
    }
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent == Path::new(repo_dir) {
            return false;
        }
        if fs::symlink_metadata(parent).is_ok_and(|metadata| !metadata.is_dir()) {
            return true;
        }
        current = parent.parent();
    }
    false
}

fn format_capture_error(stash_ref: Option<&str>, message: &str, error: &str) -> String {
    let handle = stash_ref
        .map(|stash| format!("stash commit {stash}"))
        .unwrap_or_else(|| format!("stash message {message} (search git stash list)"));
    format!("{error}; recovery handle: {handle}")
}

fn first_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("unknown git error")
        .to_string()
}
