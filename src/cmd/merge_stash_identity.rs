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

pub(crate) fn push_stash(repo_dir: &str, message: &str) -> Result<String, String> {
    // `stash create` cannot include untracked files; push -u stores both kinds durably at once.
    let output = Command::new("git")
        .args([
            "-C",
            repo_dir,
            "stash",
            "push",
            "--include-untracked",
            "--message",
            message,
        ])
        .output()
        .map_err(|error| format!("failed to capture merge-local changes: {error}"))?;
    if output.status.success() {
        let prefix = "Saved working directory and index state ";
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::to_string))
            .ok_or_else(|| "git stash did not report its exact identity".to_string())
    } else {
        Err(format!(
            "failed to capture merge-local changes: {}",
            first_error_line(&output.stderr)
        ))
    }
}

pub(crate) fn find_stash(repo_dir: &str, expected_subject: &str) -> Result<String, String> {
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

fn first_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("unknown git error")
        .to_string()
}
