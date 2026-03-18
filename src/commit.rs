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
    // Only stage tracked files that were modified — avoid committing aid-injected
    // temp files (batch TOML, team knowledge, shared context) via `git add -u`.
    let add = Command::new("git").args(["-C", dir, "add", "-u"]).output().context("Failed to run git add")?;
    anyhow::ensure!(add.status.success(), "git add failed: {}", String::from_utf8_lossy(&add.stderr));
    // Also stage new source files the agent created, but not aid artifacts.
    let _ = Command::new("git").args(["-C", dir, "add", "src/", "tests/", "crates/"]).output();
    let clean = strip_aid_tags(prompt);
    // Skip injected context prefixes like [Shared Context: ...] and [Team Knowledge — ...]
    let summary = extract_task_summary(&clean);
    let commit = Command::new("git").args(["-C", dir, "commit", "--allow-empty-message", "-m", &format!("{summary}\n\nTask: {task_id}")]).output().context("Failed to run git commit")?;
    anyhow::ensure!(commit.status.success(), "git commit failed: {}", String::from_utf8_lossy(&commit.stderr));
    Ok(())
}

/// Extract the actual task description, skipping injected context headers.
fn extract_task_summary(prompt: &str) -> String {
    for line in prompt.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('[')
            || trimmed.starts_with("---")
            || trimmed.starts_with('#')
        {
            continue;
        }
        return trimmed.chars().take(60).collect();
    }
    prompt.chars().take(60).collect()
}

/// Strip `<aid-*>...</aid-*>` tag blocks from text to prevent prompt metadata
/// from leaking into commit messages and other user-visible outputs.
fn strip_aid_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut inside_tag = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<aid-") && trimmed.ends_with('>') && !trimmed.starts_with("</") {
            inside_tag = true;
            continue;
        }
        if trimmed.starts_with("</aid-") && trimmed.ends_with('>') {
            inside_tag = false;
            continue;
        }
        if !inside_tag {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{extract_task_summary, has_uncommitted_changes, strip_aid_tags};
    use crate::test_subprocess;

    #[test]
    fn strip_aid_tags_removes_tag_blocks() {
        let input = "Implement feature X\n<aid-team-rules>\nDo not format\nOnly add modified files\n</aid-team-rules>\nExtra context here";
        let result = strip_aid_tags(input);
        assert_eq!(result, "Implement feature X\nExtra context here");
    }

    #[test]
    fn strip_aid_tags_handles_multiple_blocks() {
        let input = "<aid-project-rules>\nrule1\n</aid-project-rules>\nDo the thing\n<aid-team-rules>\nrule2\n</aid-team-rules>";
        let result = strip_aid_tags(input);
        assert_eq!(result, "Do the thing");
    }

    #[test]
    fn strip_aid_tags_passthrough_no_tags() {
        let input = "Just a normal prompt with no tags";
        assert_eq!(strip_aid_tags(input), input);
    }

    #[test]
    fn extract_task_summary_skips_context_headers() {
        let prompt = "[Shared Context: batch]\nSome shared stuff\n\n[Team Knowledge — dev]\n- coding rules\n\n[Task]\nImplement the parser changes for v2";
        assert_eq!(extract_task_summary(prompt), "Some shared stuff");
    }

    #[test]
    fn extract_task_summary_plain_prompt() {
        assert_eq!(extract_task_summary("Fix the login bug"), "Fix the login bug");
    }

    #[test]
    fn detects_dirty_git_repo() {
        let _permit = test_subprocess::acquire();
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init"]).status().unwrap().success());
        assert!(!has_uncommitted_changes(dir.path().to_str().unwrap()).unwrap());
        std::fs::write(dir.path().join("tracked.txt"), "change").unwrap();
        assert!(has_uncommitted_changes(dir.path().to_str().unwrap()).unwrap());
    }
}
