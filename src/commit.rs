// Git commit helpers for worktree task cleanup.
// Exports dirty-state detection and automatic task commits via `git`.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

mod rescue;
pub use rescue::{RescueOutcome, rescue_dirty_worktree};
#[allow(unused_imports)]
pub use rescue::{detect_untracked_source_files, rescue_untracked_files};
use rescue::stage_untracked_source_files;

pub fn has_uncommitted_changes(dir: &str) -> Result<bool> {
    Ok(crate::worktree::capture_worktree_snapshot(Path::new(dir))?.has_uncommitted_changes())
}

pub fn head_sha(dir: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["-C", dir, "rev-parse", "HEAD"])
        .output()
        .context("Failed to check git HEAD")?;
    anyhow::ensure!(out.status.success(), "git rev-parse HEAD failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn auto_commit(dir: &str, task_id: &str, prompt: &str) -> Result<()> {
    // Only stage tracked files that were modified — avoid committing aid-injected
    // temp files (batch TOML, team knowledge, shared context) via `git add -u`.
    if head_sha(dir).is_ok() {
        let add = Command::new("git")
            .args(["-C", dir, "add", "-u", "--", ".", ":(exclude).aid-lock", ":(exclude)result-*.md", ":(exclude)aid-batch-*.toml"])
            .output()
            .context("Failed to run git add")?;
        anyhow::ensure!(add.status.success(), "git add failed: {}", String::from_utf8_lossy(&add.stderr));
    }
    // Also stage new source files the agent created, but not aid artifacts.
    stage_untracked_source_files(dir, task_id)?;
    if !has_staged_changes(dir)? {
        return Ok(());
    }
    let clean = strip_aid_tags(prompt);
    // Skip injected context prefixes like [Shared Context: ...] and [Team Knowledge — ...]
    let summary = extract_task_summary(&clean);
    let commit = Command::new("git").args(["-C", dir, "commit", "--allow-empty-message", "-m", &format!("{summary}\n\nTask: {task_id}")]).output().context("Failed to run git commit")?;
    anyhow::ensure!(commit.status.success(), "git commit failed: {}", String::from_utf8_lossy(&commit.stderr));
    Ok(())
}

fn has_staged_changes(dir: &str) -> Result<bool> {
    let out = Command::new("git")
        .args(["-C", dir, "diff", "--cached", "--quiet"])
        .output()
        .context("Failed to run git diff --cached --quiet")?;
    match out.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => anyhow::bail!("git diff --cached --quiet failed: {}", String::from_utf8_lossy(&out.stderr)),
    }
}
/// Extract the actual task description from the [Task] section, falling back to
/// the first non-header content line.
fn extract_task_summary(prompt: &str) -> String {
    // Prefer content after [Task] header — that's the real task description
    let mut in_task_section = false;
    for line in prompt.lines() {
        let trimmed = line.trim();
        if trimmed == "[Task]" {
            in_task_section = true;
            continue;
        }
        if in_task_section {
            if trimmed.is_empty()
                || trimmed.starts_with('[')
                || trimmed.starts_with("---")
                || trimmed.starts_with('#')
            {
                continue;
            }
            return trimmed.chars().take(60).collect();
        }
    }
    // Fallback: first non-header content line
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
    use super::{auto_commit, extract_task_summary, has_uncommitted_changes, strip_aid_tags};
    use crate::test_subprocess;
    use std::{path::Path, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
    }

    fn git_stdout(dir: &Path, args: &[&str]) -> String {
        String::from_utf8(Command::new("git").arg("-C").arg(dir).args(args).output().unwrap().stdout).unwrap()
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test User"]);
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        dir
    }

    fn write_path(dir: &Path, path: &str, content: &str) {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(file, content).unwrap();
    }

    fn commit_path(dir: &Path, path: &str, content: &str) {
        write_path(dir, path, content);
        git(dir, &["add", path]);
        git(dir, &["commit", "-m", "initial"]);
    }

    fn head(dir: &Path) -> String {
        git_stdout(dir, &["rev-parse", "HEAD"])
    }

    #[test]
    fn strip_aid_tags_removes_tag_blocks() { let input = "Implement feature X\n<aid-team-rules>\nDo not format\nOnly add modified files\n</aid-team-rules>\nExtra context here"; assert_eq!(strip_aid_tags(input), "Implement feature X\nExtra context here"); }

    #[test]
    fn strip_aid_tags_handles_multiple_blocks() { let input = "<aid-project-rules>\nrule1\n</aid-project-rules>\nDo the thing\n<aid-team-rules>\nrule2\n</aid-team-rules>"; assert_eq!(strip_aid_tags(input), "Do the thing"); }

    #[test]
    fn strip_aid_tags_passthrough_no_tags() { let input = "Just a normal prompt with no tags"; assert_eq!(strip_aid_tags(input), input); }

    #[test]
    fn extract_task_summary_prefers_task_section() { let prompt = "[Shared Context: batch]\nAuto-created for batch dispatch\n\n[Team Knowledge — dev]\n- coding rules\n\n[Task]\nImplement the parser changes for v2"; assert_eq!(extract_task_summary(prompt), "Implement the parser changes for v2"); }

    #[test]
    fn extract_task_summary_plain_prompt() { assert_eq!(extract_task_summary("Fix the login bug"), "Fix the login bug"); }

    #[test]
    fn detects_dirty_git_repo() { let _permit = test_subprocess::acquire(); let dir = repo(); assert!(!has_uncommitted_changes(dir.path().to_str().unwrap()).unwrap()); write_path(dir.path(), "tracked.txt", "change"); assert!(has_uncommitted_changes(dir.path().to_str().unwrap()).unwrap()); }

    #[test]
    fn auto_commit_succeeds_on_repo_without_head() { let _permit = test_subprocess::acquire(); let dir = repo(); write_path(dir.path(), "first.txt", "hello"); auto_commit(dir.path().to_str().unwrap(), "task-123", "[Task]\nCreate the first file").unwrap(); assert!(!head(dir.path()).is_empty()); assert_eq!(git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]).trim(), "first.txt"); }

    #[test]
    fn auto_commit_skips_when_only_aid_lock_changed() { let _permit = test_subprocess::acquire(); let dir = repo(); commit_path(dir.path(), ".aid-lock", "initial"); let before = head(dir.path()); write_path(dir.path(), ".aid-lock", "changed"); auto_commit(dir.path().to_str().unwrap(), "task-123", "[Task]\nIgnore lock").unwrap(); assert_eq!(head(dir.path()), before); }

    #[test]
    fn auto_commit_commits_real_source_changes() { let _permit = test_subprocess::acquire(); let dir = repo(); commit_path(dir.path(), "src/main.rs", "fn main() {}\n"); let before = head(dir.path()); write_path(dir.path(), "src/main.rs", "fn main() { println!(\"changed\"); }\n"); auto_commit(dir.path().to_str().unwrap(), "task-123", "[Task]\nChange source").unwrap(); assert_ne!(head(dir.path()), before); }

    #[test]
    fn auto_commit_ignores_result_files() { let _permit = test_subprocess::acquire(); let dir = repo(); commit_path(dir.path(), "src/main.rs", "fn main() {}\n"); commit_path(dir.path(), "result-t-1234.md", "transient"); write_path(dir.path(), "result-t-1234.md", "changed"); write_path(dir.path(), "src/main.rs", "fn main() { println!(\"changed\"); }\n"); auto_commit(dir.path().to_str().unwrap(), "task-123", "[Task]\nChange source").unwrap(); let changed = git_stdout(dir.path(), &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]); assert!(changed.lines().any(|line| line == "src/main.rs")); assert!(!changed.lines().any(|line| line == "result-t-1234.md")); }

}
