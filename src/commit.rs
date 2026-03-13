// Git commit helpers for worktree task cleanup.
// Exports dirty-state detection and automatic task commits via `git`.

use anyhow::{Context, Result};
use std::process::Command;

pub fn has_uncommitted_changes(dir: &str) -> Result<bool> {
    let out = Command::new("git").args(["-C", dir, "status", "--porcelain"]).output().context("Failed to run git status")?;
    anyhow::ensure!(out.status.success(), "git status failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(!out.stdout.is_empty())
}

pub fn auto_commit(dir: &str, task_id: &str, prompt: &str) -> Result<()> {
    let add = Command::new("git").args(["-C", dir, "add", "-A"]).output().context("Failed to run git add")?;
    anyhow::ensure!(add.status.success(), "git add failed: {}", String::from_utf8_lossy(&add.stderr));
    let summary = prompt.chars().take(60).collect::<String>();
    let commit = Command::new("git").args(["-C", dir, "commit", "-m", &format!("feat: {summary}\n\nTask: {task_id}")]).output().context("Failed to run git commit")?;
    anyhow::ensure!(commit.status.success(), "git commit failed: {}", String::from_utf8_lossy(&commit.stderr));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::has_uncommitted_changes;

    #[test]
    fn detects_dirty_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init"]).status().unwrap().success());
        assert!(!has_uncommitted_changes(dir.path().to_str().unwrap()).unwrap());
        std::fs::write(dir.path().join("tracked.txt"), "change").unwrap();
        assert!(has_uncommitted_changes(dir.path().to_str().unwrap()).unwrap());
    }
}
