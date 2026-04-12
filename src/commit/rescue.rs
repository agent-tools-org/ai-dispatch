// Dirty worktree rescue for agent commits.
// Exports rescue outcome types plus untracked/modified staging helpers.
// Deps: git CLI via std::process and parent commit helpers.

use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescueOutcome {
    pub staged: Vec<String>,
    pub committed: bool,
    pub had_existing_head: bool,
    pub error: Option<String>,
    pub untracked: Vec<String>,
    pub modified: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirtyKind {
    Untracked,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirtyFile {
    path: String,
    kind: DirtyKind,
}

pub fn detect_untracked_source_files(dir: &str) -> Result<Vec<String>> {
    Ok(detect_rescuable_files(dir)?
        .into_iter()
        .filter(|file| file.kind == DirtyKind::Untracked)
        .map(|file| file.path)
        .collect())
}

pub fn rescue_dirty_worktree(dir: &str, task_id: &str) -> Result<RescueOutcome> {
    let had_existing_head = crate::commit::head_sha(dir).is_ok();
    let files = detect_rescuable_files(dir)?;
    let mut outcome = RescueOutcome {
        staged: Vec::new(),
        committed: false,
        had_existing_head,
        error: None,
        untracked: Vec::new(),
        modified: Vec::new(),
    };
    for file in files {
        if let Err(err) = stage_file(dir, &file) {
            outcome.error = Some(err);
            return Ok(outcome);
        }
        outcome.staged.push(file.path.clone());
        match file.kind {
            DirtyKind::Untracked => outcome.untracked.push(file.path),
            DirtyKind::Modified => outcome.modified.push(file.path),
        }
    }
    if outcome.staged.is_empty() {
        return Ok(outcome);
    }
    if let Err(err) = commit_rescue(dir, task_id, had_existing_head) {
        outcome.error = Some(err);
        return Ok(outcome);
    }
    outcome.committed = true;
    Ok(outcome)
}

#[allow(dead_code)]
pub fn rescue_untracked_files(dir: &str, task_id: &str) -> Result<Vec<String>> {
    Ok(rescue_dirty_worktree(dir, task_id)?.untracked)
}

pub(super) fn stage_untracked_source_files(dir: &str, task_id: &str) -> Result<Vec<String>> {
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
            aid_warn!("[aid] Warning: failed to stage rescued file for {task_id}: {}", first_stderr_line(&add.stderr));
            break;
        }
        staged.push(file);
    }
    Ok(staged)
}

fn detect_rescuable_files(dir: &str) -> Result<Vec<DirtyFile>> {
    let out = Command::new("git").args(["-C", dir, "status", "--porcelain"]).output().context("Failed to run git status")?;
    anyhow::ensure!(out.status.success(), "git status failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_dirty_line)
        .filter(|file| should_rescue_path(&file.path))
        .collect())
}

fn parse_dirty_line(line: &str) -> Option<DirtyFile> {
    if let Some(path) = line.strip_prefix("?? ") {
        return Some(DirtyFile { path: path.to_string(), kind: DirtyKind::Untracked });
    }
    if line.len() < 4 {
        return None;
    }
    let status = &line[..2];
    let path = &line[3..];
    if status.contains('M') {
        return Some(DirtyFile { path: path.to_string(), kind: DirtyKind::Modified });
    }
    None
}

fn should_rescue_path(path: &str) -> bool {
    !["target/", "node_modules/", "__pycache__/", ".aid-", "aid-batch-"].iter().any(|part| path.contains(part))
        && ![".pyc", ".pyo", ".class", ".o", ".so", ".dylib"].iter().any(|suffix| path.ends_with(suffix))
        && !(path.starts_with("result-") && path.ends_with(".md"))
}

fn stage_file(dir: &str, file: &DirtyFile) -> std::result::Result<(), String> {
    let mut add = Command::new("git");
    add.args(["-C", dir]);
    match file.kind {
        DirtyKind::Untracked => {
            add.args(["add", "--"]);
        }
        DirtyKind::Modified => {
            add.args(["add", "-u", "--"]);
        }
    }
    let output = add.arg(&file.path).output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_stderr_line(&output.stderr))
    }
}

fn commit_rescue(dir: &str, task_id: &str, had_existing_head: bool) -> std::result::Result<(), String> {
    let output = if had_existing_head {
        Command::new("git").args(["-C", dir, "commit", "--amend", "--no-edit"]).output()
    } else {
        Command::new("git")
            .args(["-C", dir, "commit", "-m", &format!("[aid] rescue: stage files missed by agent (task: {task_id})")])
            .output()
    }
    .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_stderr_line(&output.stderr))
    }
}

fn first_stderr_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::{detect_untracked_source_files, rescue_dirty_worktree, rescue_untracked_files};
    use crate::test_subprocess;
    use std::{path::Path, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
    }

    fn git_stdout(dir: &Path, args: &[&str]) -> String {
        String::from_utf8(Command::new("git").arg("-C").arg(dir).args(args).output().unwrap().stdout).unwrap()
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test User"]);
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
    fn detect_untracked_finds_new_source_files() {
        let _permit = test_subprocess::acquire();
        let dir = repo();
        write_path(dir.path(), "new_file.rs", "fn main() {}\n");
        assert_eq!(detect_untracked_source_files(dir.path().to_str().unwrap()).unwrap(), vec!["new_file.rs"]);
    }

    #[test]
    fn detect_untracked_ignores_artifacts() {
        let _permit = test_subprocess::acquire();
        let dir = repo();
        for path in ["target/out.rs", "node_modules/pkg.js", "__pycache__/mod.pyc", ".aid-temp.rs", "aid-batch-note.ts", "native.so", "result-t-1234.md"] {
            write_path(dir.path(), path, "x");
        }
        assert!(detect_untracked_source_files(dir.path().to_str().unwrap()).unwrap().is_empty());
    }

    #[test]
    fn rescue_untracked_amends_commit() {
        let _permit = test_subprocess::acquire();
        let dir = repo();
        commit_path(dir.path(), "tracked.txt", "tracked");
        write_path(dir.path(), "rescued.rs", "pub fn rescued() {}\n");
        assert_eq!(rescue_untracked_files(dir.path().to_str().unwrap(), "task-123").unwrap(), vec!["rescued.rs"]);
        let tree = git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
        assert!(tree.lines().any(|line| line == "rescued.rs"));
    }

    #[test]
    fn rescue_dirty_worktree_stages_modified_file() {
        let _permit = test_subprocess::acquire();
        let dir = repo();
        commit_path(dir.path(), "src/main.rs", "fn main() {}\n");
        let before = head(dir.path());
        write_path(dir.path(), "src/main.rs", "fn main() { println!(\"changed\"); }\n");
        let outcome = rescue_dirty_worktree(dir.path().to_str().unwrap(), "task-123").unwrap();
        assert_eq!(outcome.modified, vec!["src/main.rs"]);
        assert!(outcome.committed);
        assert!(outcome.had_existing_head);
        assert_ne!(head(dir.path()), before);
        assert!(git_stdout(dir.path(), &["show", "HEAD:src/main.rs"]).contains("changed"));
    }

    #[test]
    fn rescue_dirty_worktree_creates_initial_commit_when_no_head() {
        let _permit = test_subprocess::acquire();
        let dir = repo();
        write_path(dir.path(), "src/lib.rs", "pub fn value() -> u8 { 1 }\n");
        let outcome = rescue_dirty_worktree(dir.path().to_str().unwrap(), "task-123").unwrap();
        assert_eq!(outcome.untracked, vec!["src/lib.rs"]);
        assert!(outcome.committed);
        assert!(!outcome.had_existing_head);
        assert_eq!(git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]).trim(), "src/lib.rs");
    }

    #[test]
    fn rescue_dirty_worktree_respects_exclusions() {
        let _permit = test_subprocess::acquire();
        let dir = repo();
        write_path(dir.path(), "target/foo.rs", "ignored");
        write_path(dir.path(), "src/bar.rs", "pub fn bar() {}\n");
        let outcome = rescue_dirty_worktree(dir.path().to_str().unwrap(), "task-123").unwrap();
        assert_eq!(outcome.staged, vec!["src/bar.rs"]);
        let tree = git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
        assert!(tree.lines().any(|line| line == "src/bar.rs"));
        assert!(!tree.lines().any(|line| line == "target/foo.rs"));
    }
}
