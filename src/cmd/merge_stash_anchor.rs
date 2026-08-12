// Owns merge-local Git refs and removes recovery anchors after expiry.
// Exports anchor creation, exact cleanup, and stale-anchor sweeping.
// Deps: std::process::Command and system time.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const ANCHOR_ROOT: &str = "refs/aid/merge-local";
const ANCHOR_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;

pub(crate) fn anchor_tracked_commit(repo_dir: &str, commit: &str) -> Result<String, String> {
    let anchor = format!("{ANCHOR_ROOT}/{commit}");
    let output = Command::new("git")
        .args(["-C", repo_dir, "update-ref", &anchor, commit])
        .output()
        .map_err(|error| format!("failed to retain tracked changes {commit}: {error}"))?;
    if output.status.success() {
        Ok(anchor)
    } else {
        Err(format!(
            "failed to retain tracked changes {commit}: {}",
            first_error_line(&output.stderr)
        ))
    }
}

pub(crate) fn drop_tracked_anchor(
    repo_dir: &str,
    anchor: &str,
    commit: &str,
) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "update-ref", "-d", anchor, commit])
        .output()
        .map_err(|error| format!("failed to drop tracked-change ref {anchor}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to drop tracked-change ref {anchor}: {}",
            first_error_line(&output.stderr)
        ))
    }
}

pub(crate) fn sweep_stale_anchors(repo_dir: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "for-each-ref", "--format=%(refname)%09%(creatordate:unix)", ANCHOR_ROOT])
        .output()
        .map_err(|error| format!("failed to inspect merge recovery refs: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to inspect merge recovery refs: {}",
            first_error_line(&output.stderr)
        ));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to inspect merge recovery refs: {error}"))?
        .as_secs() as i64;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((anchor, created)) = line.split_once('\t') else { continue };
        let Ok(created) = created.parse::<i64>() else { continue };
        if now.saturating_sub(created) > ANCHOR_MAX_AGE_SECS {
            drop_stale_anchor(repo_dir, anchor)?;
        }
    }
    Ok(())
}

fn drop_stale_anchor(repo_dir: &str, anchor: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "update-ref", "-d", anchor])
        .output()
        .map_err(|error| format!("failed to expire merge recovery ref {anchor}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to expire merge recovery ref {anchor}: {}",
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
