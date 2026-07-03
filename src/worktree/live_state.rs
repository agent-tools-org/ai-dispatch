// Live git worktree status and dirty-stat rendering.
// Exports status counts plus a capture boundary for show, salvage, and prune.
// Deps: git CLI via std::process, anyhow, std::path.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeStatusSummary {
    pub(crate) modified: usize,
    pub(crate) staged: usize,
    pub(crate) untracked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveWorktreeState {
    pub(crate) status_lines: Vec<String>,
    pub(crate) summary: WorktreeStatusSummary,
    unstaged_stat: String,
    staged_stat: String,
}

impl LiveWorktreeState {
    pub(crate) fn is_dirty(&self) -> bool {
        !self.status_lines.is_empty()
    }

    pub(crate) fn summary_text(&self) -> String {
        format!(
            "modified: {}, staged: {}, untracked: {}",
            self.summary.modified, self.summary.staged, self.summary.untracked
        )
    }

    pub(crate) fn dirty_stat_text(&self) -> String {
        let mut out = String::new();
        if !self.status_lines.is_empty() {
            out.push_str("Status names:\n");
            for line in &self.status_lines {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        push_stat_block(&mut out, "Unstaged diff stat", &self.unstaged_stat);
        push_stat_block(&mut out, "Staged diff stat", &self.staged_stat);
        out
    }
}

pub(crate) fn capture_live_worktree_state(dir: &Path) -> Result<LiveWorktreeState> {
    let status_lines = read_status_lines(dir)?;
    let summary = summarize_status(&status_lines);
    let unstaged_stat = git_output(dir, &["diff", "--stat"])?;
    let staged_stat = git_output(dir, &["diff", "--cached", "--stat"])?;
    Ok(LiveWorktreeState {
        status_lines,
        summary,
        unstaged_stat,
        staged_stat,
    })
}

pub(crate) fn worktree_has_uncommitted_changes(dir: &Path) -> Result<bool> {
    Ok(!read_status_lines(dir)?.is_empty())
}

pub(crate) fn uncommitted_diff_text(dir: &Path) -> Result<String> {
    let unstaged = git_output(dir, &["diff"])?;
    let staged = git_output(dir, &["diff", "--cached"])?;
    let mut out = String::new();
    if !unstaged.trim().is_empty() {
        out.push_str(&unstaged);
        if !unstaged.ends_with('\n') {
            out.push('\n');
        }
    }
    if !staged.trim().is_empty() {
        out.push_str(&staged);
    }
    Ok(out)
}

fn summarize_status(lines: &[String]) -> WorktreeStatusSummary {
    let mut summary = WorktreeStatusSummary {
        modified: 0,
        staged: 0,
        untracked: 0,
    };
    for line in lines {
        if line.starts_with("?? ") {
            summary.untracked += 1;
            continue;
        }
        let mut chars = line.chars();
        let index = chars.next().unwrap_or(' ');
        let worktree = chars.next().unwrap_or(' ');
        if index != ' ' {
            summary.staged += 1;
        }
        if worktree != ' ' {
            summary.modified += 1;
        }
    }
    summary
}

fn read_status_lines(dir: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .context("Failed to run git status")?;
    anyhow::ensure!(
        output.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn push_stat_block(out: &mut String, label: &str, stat: &str) {
    if stat.trim().is_empty() {
        return;
    }
    out.push_str(label);
    out.push_str(":\n");
    out.push_str(stat);
    if !stat.ends_with('\n') {
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::{capture_live_worktree_state, worktree_has_uncommitted_changes};
    use crate::test_subprocess;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git").arg("-C").arg(dir).args(args).status().expect("git");
        assert!(status.success());
    }

    fn repo() -> tempfile::TempDir {
        let _permit = test_subprocess::acquire();
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test User"]);
        std::fs::write(dir.path().join("tracked.txt"), "base\n").expect("write");
        git(dir.path(), &["add", "tracked.txt"]);
        git(dir.path(), &["commit", "-m", "base"]);
        dir
    }

    #[test]
    fn live_state_counts_modified_staged_and_untracked_files() {
        let dir = repo();
        std::fs::write(dir.path().join("tracked.txt"), "changed\n").expect("write");
        std::fs::write(dir.path().join("staged.txt"), "staged\n").expect("write");
        std::fs::write(dir.path().join("new.txt"), "new\n").expect("write");
        git(dir.path(), &["add", "staged.txt"]);

        let state = capture_live_worktree_state(dir.path()).expect("state");

        assert_eq!(state.summary.modified, 1);
        assert_eq!(state.summary.staged, 1);
        assert_eq!(state.summary.untracked, 1);
        assert!(state.dirty_stat_text().contains("?? new.txt"));
        assert!(worktree_has_uncommitted_changes(dir.path()).expect("dirty"));
    }
}
