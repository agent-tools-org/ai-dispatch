// Identifies and manages durable merge-local stash entries.
// Exports exact message lookup, SHA-based apply, and checked cleanup.
// Deps: Git stash plumbing and standard time handling.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unique_stash_message() -> Result<String, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to create merge stash identity: {error}"))?
        .as_nanos();
    Ok(format!("aid merge-local {}-{timestamp}", std::process::id()))
}

pub(crate) fn push_stash(repo_dir: &str, message: &str) -> Result<(), String> {
    // `stash create` cannot include untracked files; push -u stores both kinds durably at once.
    let output = Command::new("git")
        .args([
            "-C", repo_dir, "stash", "push", "--include-untracked", "--quiet", "--message",
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

pub(crate) fn find_stash(repo_dir: &str, message: &str) -> Result<String, String> {
    let expected_subject = stash_subject(repo_dir, message)?;
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
    let mut matches = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((commit, subject)) = line.split_once('\t') else {
            continue;
        };
        if subject == expected_subject {
            matches.push(commit.to_string());
        }
    }
    match matches.as_slice() {
        [stash_ref] => Ok(stash_ref.clone()),
        [] => Err(format!("stash identity {expected_subject} was not found in git stash list")),
        _ => Err(format!(
            "multiple stashes matched identity {expected_subject}: {}",
            matches.join(", ")
        )),
    }
}

fn stash_subject(repo_dir: &str, message: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "branch", "--show-current"])
        .output()
        .map_err(|error| format!("failed to identify merge stash branch: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to identify merge stash branch: {}",
            first_error_line(&output.stderr)
        ));
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        Ok(format!("WIP on (no branch): {message}"))
    } else {
        Ok(format!("On {branch}: {message}"))
    }
}

pub(crate) fn apply_stash(repo_dir: &str, stash_ref: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "apply", "--index", stash_ref])
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

pub(crate) fn drop_stash_if_exact<F>(
    repo_dir: &str,
    stash_ref: &str,
    before_drop: F,
) -> Result<(), String>
where
    F: FnOnce(),
{
    let selector = find_stash_selector(repo_dir, stash_ref)?;
    verify_stash_selector(repo_dir, &selector, stash_ref)?;
    before_drop();
    verify_stash_selector(repo_dir, &selector, stash_ref)?;
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "drop", "--quiet", &selector])
        .output()
        .map_err(|error| format!("failed to drop merge-local stash {stash_ref}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to drop merge-local stash {stash_ref}: {}",
            first_error_line(&output.stderr)
        ))
    }
}

fn find_stash_selector(repo_dir: &str, stash_ref: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "list", "--format=%H%x09%gd"])
        .output()
        .map_err(|error| format!("failed to locate merge-local stash {stash_ref}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to locate merge-local stash {stash_ref}: {}",
            first_error_line(&output.stderr)
        ));
    }
    let mut matches = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((commit, selector)) = line.split_once('\t') else {
            continue;
        };
        if commit == stash_ref {
            matches.push(selector.to_string());
        }
    }
    match matches.as_slice() {
        [selector] => Ok(selector.clone()),
        [] => Err(format!("stash commit {stash_ref} is no longer in git stash list")),
        _ => Err(format!("stash commit {stash_ref} has multiple list entries")),
    }
}

fn verify_stash_selector(repo_dir: &str, selector: &str, expected: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "--verify", selector])
        .output()
        .map_err(|error| format!("failed to verify merge-local stash {expected}: {error}"))?;
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && actual == expected {
        Ok(())
    } else {
        Err(format!(
            "refusing to drop merge-local stash {expected}: {selector} changed"
        ))
    }
}

fn first_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("unknown git error")
        .to_string()
}
