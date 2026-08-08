// Keeps aid's own runtime files out of the target repo's git status.
// Exports ensure_aid_paths_excluded, called when aid creates a worktree.
// Deps: git CLI via std::process, std::fs.

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Written into the repo's local exclude file, never into a tracked `.gitignore`.
const AID_EXCLUDE_ENTRIES: &[&str] =
    &[".aid-*", "aid-batch-*", "result-t-*.md", "result-t-*.json", ".aid/state.toml", ".aid/batches/"];

const AID_EXCLUDE_HEADER: &str = "# aid: runtime files written into this repo by ai-dispatch";

/// Teach git to ignore the files aid writes into a checkout.
///
/// Without this an agent running `git add .` commits aid's `.aid-lock` and
/// `.aid-verify-deps-state`; once tracked, aid removing its own lock at task end shows
/// up as ` D .aid-lock` and reads as the agent leaving work behind. Repos hit by this
/// have carried hand-written "ignore aid's worktree bookkeeping files" commits.
///
/// Writes to `$GIT_COMMON_DIR/info/exclude` — local to the clone, never committed, and
/// the only exclude file git actually consults. A per-worktree `info/exclude` under
/// `.git/worktrees/<id>/` is silently ignored by git, so this deliberately does not
/// try to scope the entries to one worktree.
///
/// Never fails a dispatch: a repo that cannot be written to still runs, it just keeps
/// the old noise.
pub fn ensure_aid_paths_excluded(dir: &Path) {
    if let Err(err) = write_exclude_entries(dir) {
        aid_warn!("[aid] could not add aid's runtime files to {}'s git exclude: {err}", dir.display());
    }
}

fn write_exclude_entries(dir: &Path) -> Result<()> {
    let common_dir = git_common_dir(dir)?;
    let exclude_path = common_dir.join("info").join("exclude");
    let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    let missing: Vec<&str> = AID_EXCLUDE_ENTRIES
        .iter()
        .filter(|entry| !existing.lines().any(|line| line.trim() == **entry))
        .copied()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.contains(AID_EXCLUDE_HEADER) {
        out.push_str(AID_EXCLUDE_HEADER);
        out.push('\n');
    }
    for entry in missing {
        out.push_str(entry);
        out.push('\n');
    }
    std::fs::write(&exclude_path, out)?;
    Ok(())
}

fn git_common_dir(dir: &Path) -> Result<std::path::PathBuf> {
    let output = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "rev-parse", "--git-common-dir"])
        .output()?;
    anyhow::ensure!(output.status.success(), "git rev-parse --git-common-dir failed");
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    anyhow::ensure!(!raw.is_empty(), "git reported no common dir");
    let path = std::path::PathBuf::from(&raw);
    Ok(if path.is_absolute() { path } else { dir.join(path) })
}

#[cfg(test)]
#[path = "exclude_tests.rs"]
mod tests;
