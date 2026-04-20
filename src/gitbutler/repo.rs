// GitButler repo detection helpers used by batch prompts and merge hints.
// Exports repo marker detection via the resolved git dir for normal repos and worktrees.
// Deps: anyhow, std::path, std::process::Command.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn repo_has_markers(repo_dir: &Path) -> bool {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AID_GITBUTLER_TEST_REPO_MARKERS") {
        return matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    }

    let Ok(git_dir) = resolve_git_dir(repo_dir) else {
        return false;
    };
    git_dir.join("gitbutler").is_dir() || git_dir.join("virtual_branches.toml").is_file()
}

fn resolve_git_dir(repo_dir: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .context("Failed to run git rev-parse --absolute-git-dir")?;
    anyhow::ensure!(
        output.status.success(),
        "git rev-parse --absolute-git-dir failed for {}",
        repo_dir.display()
    );
    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    anyhow::ensure!(!git_dir.is_empty(), "Resolved git dir is empty");
    Ok(PathBuf::from(git_dir))
}
