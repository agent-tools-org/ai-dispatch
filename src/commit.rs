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
    let has_head = Command::new("git")
        .args(["-C", dir, "rev-parse", "HEAD"])
        .output()
        .context("Failed to check git HEAD")?
        .status
        .success();
    let add_mode = if has_head { "-u" } else { "-A" };
    let add = Command::new("git")
        .args(["-C", dir, "add", add_mode])
        .output()
        .context("Failed to run git add")?;
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
pub fn detect_untracked_source_files(dir: &str) -> Result<Vec<String>> {
    let out = Command::new("git").args(["-C", dir, "status", "--porcelain"]).output().context("Failed to run git status")?;
    anyhow::ensure!(out.status.success(), "git status failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("?? "))
        .filter(|path| {
            !["target/", "node_modules/", "__pycache__/", ".aid-", "aid-batch-"].iter().any(|part| path.contains(part))
                && ![".pyc", ".pyo", ".class", ".o", ".so", ".dylib"].iter().any(|suffix| path.ends_with(suffix))
        })
        .map(str::to_owned)
        .collect())
}

pub fn rescue_untracked_files(dir: &str, task_id: &str) -> Result<Vec<String>> {
    let mut staged = Vec::new();
    let files = match detect_untracked_source_files(dir) {
        Ok(files) => files,
        Err(err) => {
            aid_warn!("[aid] Warning: failed to detect untracked files for {task_id}: {err}");
            return Ok(Vec::new());
        }
    };
    for file in files {
        let add = match Command::new("git").args(["-C", dir, "add", "--"]).arg(&file).output() {
            Ok(add) => add,
            Err(err) => {
                aid_warn!("[aid] Warning: failed to stage rescued file for {task_id}: {err}");
                break;
            }
        };
        if !add.status.success() {
            aid_warn!("[aid] Warning: failed to stage rescued file for {task_id}: {}", String::from_utf8_lossy(&add.stderr).lines().next().unwrap_or(""));
            break;
        }
        staged.push(file);
    }
    if staged.is_empty() {
        return Ok(Vec::new());
    }
    let amend = match Command::new("git").args(["-C", dir, "commit", "--amend", "--no-edit"]).output() {
        Ok(amend) => amend,
        Err(err) => {
            aid_warn!("[aid] Warning: failed to amend commit with rescued files for {task_id}: {err}");
            return Ok(Vec::new());
        }
    };
    if !amend.status.success() {
        aid_warn!("[aid] Warning: failed to amend commit with rescued files for {task_id}: {}", String::from_utf8_lossy(&amend.stderr).lines().next().unwrap_or(""));
        return Ok(Vec::new());
    }
    Ok(staged)
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
    use super::{auto_commit, detect_untracked_source_files, extract_task_summary, has_uncommitted_changes, rescue_untracked_files, strip_aid_tags};
    use crate::test_subprocess;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    fn git_stdout(dir: &std::path::Path, args: &[&str]) -> String {
        String::from_utf8(
            Command::new("git").arg("-C").arg(dir).args(args).output().unwrap().stdout,
        )
        .unwrap()
    }

    fn init_repo(dir: &std::path::Path) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test User"]);
    }
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
    fn extract_task_summary_prefers_task_section() {
        let prompt = "[Shared Context: batch]\nAuto-created for batch dispatch\n\n[Team Knowledge — dev]\n- coding rules\n\n[Task]\nImplement the parser changes for v2";
        assert_eq!(
            extract_task_summary(prompt),
            "Implement the parser changes for v2"
        );
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

    #[test]
    fn auto_commit_succeeds_on_repo_without_head() {
        let _permit = test_subprocess::acquire();
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("first.txt"), "hello").unwrap();

        auto_commit(
            dir.path().to_str().unwrap(),
            "task-123",
            "[Task]\nCreate the first file",
        )
        .unwrap();

        let head = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(head.status.success());

        let tree = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["ls-tree", "-r", "--name-only", "HEAD"])
            .output()
            .unwrap();
        assert!(tree.status.success());
        assert_eq!(String::from_utf8_lossy(&tree.stdout).trim(), "first.txt");
    }

    #[test]
    fn detect_untracked_finds_new_source_files() {
        let _permit = test_subprocess::acquire();
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("new_file.rs"), "fn main() {}\n").unwrap();
        assert_eq!(
            detect_untracked_source_files(dir.path().to_str().unwrap()).unwrap(),
            vec!["new_file.rs"]
        );
    }

    #[test]
    fn detect_untracked_ignores_artifacts() {
        let _permit = test_subprocess::acquire();
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        for path in ["target/out.rs", "node_modules/pkg.js", "__pycache__/mod.pyc", ".aid-temp.rs", "aid-batch-note.ts", "native.so"] {
            let file = dir.path().join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(file, "x").unwrap();
        }
        assert!(detect_untracked_source_files(dir.path().to_str().unwrap()).unwrap().is_empty());
    }

    #[test]
    fn rescue_untracked_amends_commit() {
        let _permit = test_subprocess::acquire();
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "tracked").unwrap();
        git(dir.path(), &["add", "tracked.txt"]);
        git(dir.path(), &["commit", "-m", "initial"]);
        std::fs::write(dir.path().join("rescued.rs"), "pub fn rescued() {}\n").unwrap();
        assert_eq!(
            rescue_untracked_files(dir.path().to_str().unwrap(), "task-123").unwrap(),
            vec!["rescued.rs"]
        );
        let tree = git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
        assert!(tree.lines().any(|line| line == "rescued.rs"));
    }
}
