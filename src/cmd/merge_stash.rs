// Stash ownership helpers for `aid merge`.
// Exports exact stash capture, restore, and recovery-error formatting.
// Deps: std::process::Command and aid logging macros.

use std::process::Command;

pub(crate) fn stash_local_changes(repo_dir: &str) -> Result<Option<String>, String> {
    let dirty = Command::new("git")
        .args(["-C", repo_dir, "status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if !dirty {
        return Ok(None);
    }
    aid_info!("[aid] Stashing local changes before merge...");
    match Command::new("git")
        .args(["-C", repo_dir, "stash", "push", "--include-untracked", "-m", "aid: auto-stash before merge"])
        .output()
    {
        Ok(o) if o.status.success() => stash_ref(repo_dir).map(Some),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(format!("failed to stash local changes: {}", stderr.lines().next().unwrap_or("unknown error")))
        }
        Err(e) => Err(format!("failed to stash local changes: {e}")),
    }
}

fn stash_ref(repo_dir: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "stash@{0}"])
        .output()
        .map_err(|error| format!("failed to identify the new stash: {error}"))?;
    if !output.status.success() {
        return Err("git created a stash but its ref could not be identified; recover it with git stash list".to_string());
    }
    let stash_ref = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stash_ref.is_empty() {
        return Err("git created a stash but returned an empty ref; recover it with git stash list".to_string());
    }
    Ok(stash_ref)
}

pub(crate) fn restore_stash(repo_dir: &str, stash_ref: &str) -> Result<(), String> {
    let selector = stash_selector(repo_dir, stash_ref)?;
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "pop", &selector])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if detail.is_empty() { "git stash pop failed".to_string() } else { detail })
}

fn stash_selector(repo_dir: &str, stash_ref: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "stash", "list", "--format=%gd%x09%H"])
        .output()
        .map_err(|error| format!("failed to locate stash {stash_ref}: {error}"))?;
    if !output.status.success() {
        return Err(format!("failed to locate stash {stash_ref}; recover it with git stash list"));
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((selector, hash)) = line.split_once('\t') else { continue };
        if hash == stash_ref {
            return Ok(selector.to_string());
        }
    }
    Err(format!("stash {stash_ref} is no longer available; recover it with git stash list"))
}

pub(crate) fn format_stash_restore_error(stash_ref: &str, error: &str) -> String {
    format!("failed to restore stash {stash_ref}: {error}; stash was kept for manual recovery")
}
