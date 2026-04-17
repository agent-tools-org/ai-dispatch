// Tests for dirty worktree rescue behavior.
// Covers amend safety, artifact exclusions, and pre-task baseline filtering.

use super::{
    detect_untracked_source_files, extract_baseline_paths, rescue_dirty_worktree,
    rescue_dirty_worktree_with_baseline, rescue_untracked_files,
};
use crate::test_subprocess;
use std::{path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
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
    git_stdout(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn commit_count(dir: &Path) -> String {
    git_stdout(dir, &["rev-list", "--count", "HEAD"])
        .trim()
        .to_string()
}

fn baseline(dir: &Path) -> Vec<String> {
    crate::worktree::capture_worktree_snapshot(dir)
        .unwrap()
        .status_lines
}

#[test]
fn detect_untracked_finds_new_source_files() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    write_path(dir.path(), "new_file.rs", "fn main() {}\n");
    assert_eq!(
        detect_untracked_source_files(dir.path().to_str().unwrap()).unwrap(),
        vec!["new_file.rs"]
    );
}

#[test]
fn detect_untracked_ignores_artifacts() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    for path in [
        "target/out.rs",
        "node_modules/pkg.js",
        "__pycache__/mod.pyc",
        ".aid-temp.rs",
        "aid-batch-note.ts",
        "native.so",
    ] {
        write_path(dir.path(), path, "x");
    }
    assert!(
        detect_untracked_source_files(dir.path().to_str().unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rescue_untracked_amends_commit() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "tracked.txt", "tracked");
    let before = head(dir.path());
    let count_before = commit_count(dir.path());
    write_path(dir.path(), "rescued.rs", "pub fn rescued() {}\n");
    assert_eq!(
        rescue_untracked_files(dir.path().to_str().unwrap(), "task-123").unwrap(),
        vec!["rescued.rs"]
    );
    assert_ne!(head(dir.path()), before);
    assert_eq!(commit_count(dir.path()), count_before);
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
    assert_eq!(
        git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]).trim(),
        "src/lib.rs"
    );
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

#[test]
fn rescue_does_not_amend_tagged_head() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "src/main.rs", "fn main() {}\n");
    let tagged_sha = head(dir.path());
    git(dir.path(), &["tag", "vtest"]);
    write_path(dir.path(), "src/rescued.rs", "pub fn rescued() {}\n");

    let outcome = rescue_dirty_worktree(dir.path().to_str().unwrap(), "task-123").unwrap();

    assert!(outcome.committed);
    assert_eq!(git_stdout(dir.path(), &["rev-parse", "vtest"]).trim(), tagged_sha);
    assert_ne!(head(dir.path()), tagged_sha);
    assert_eq!(
        git_stdout(dir.path(), &["rev-parse", "HEAD^"]).trim(),
        tagged_sha
    );
    let tree = git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.lines().any(|line| line == "src/rescued.rs"));
}

#[test]
fn rescue_preserves_pre_existing_dirty_files() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "src/existing.rs", "pub fn existing() {}\n");
    write_path(dir.path(), "src/existing.rs", "pub fn user_edit() {}\n");
    write_path(dir.path(), "src/user.rs", "pub fn user() {}\n");
    let baseline = baseline(dir.path());
    write_path(dir.path(), "src/agent.rs", "pub fn agent() {}\n");

    let outcome = rescue_dirty_worktree_with_baseline(
        dir.path().to_str().unwrap(),
        "task-123",
        Some(&baseline),
    )
    .unwrap();

    assert_eq!(outcome.staged, vec!["src/agent.rs"]);
    let tree = git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.lines().any(|line| line == "src/agent.rs"));
    assert!(!tree.lines().any(|line| line == "src/user.rs"));
    let status = git_stdout(dir.path(), &["status", "--porcelain"]);
    assert!(status.lines().any(|line| line == " M src/existing.rs"));
    assert!(status.lines().any(|line| line == "?? src/user.rs"));
}

#[test]
fn rescue_path_baseline_handles_kind_transition() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "tracked.txt", "tracked");
    write_path(dir.path(), "src/foo.rs", "pub fn user() {}\n");
    let baseline = baseline(dir.path());
    git(dir.path(), &["add", "src/foo.rs"]);
    write_path(dir.path(), "src/foo.rs", "pub fn user() {}\npub fn later() {}\n");

    let outcome = rescue_dirty_worktree_with_baseline(
        dir.path().to_str().unwrap(),
        "task-123",
        Some(&baseline),
    )
    .unwrap();

    assert!(outcome.staged.is_empty());
    assert!(!outcome.committed);
    let status = git_stdout(dir.path(), &["status", "--porcelain"]);
    assert!(status.lines().any(|line| line == "AM src/foo.rs"));
}

#[test]
fn rescue_path_baseline_handles_rename_and_delete() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "tracked.txt", "tracked");
    let baseline = vec![
        "R  src/old.rs -> src/new.rs".to_string(),
        " D src/other.rs".to_string(),
    ];

    let baseline_paths = extract_baseline_paths(&baseline);
    assert!(baseline_paths.contains("src/new.rs"));
    assert!(baseline_paths.contains("src/other.rs"));

    write_path(dir.path(), "src/new.rs", "pub fn renamed() {}\n");
    write_path(dir.path(), "src/other.rs", "pub fn deleted() {}\n");
    write_path(dir.path(), "src/agent.rs", "pub fn agent() {}\n");

    let outcome = rescue_dirty_worktree_with_baseline(
        dir.path().to_str().unwrap(),
        "task-123",
        Some(&baseline),
    )
    .unwrap();

    assert_eq!(outcome.staged, vec!["src/agent.rs"]);
    let tree = git_stdout(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.lines().any(|line| line == "src/agent.rs"));
    assert!(!tree.lines().any(|line| line == "src/new.rs"));
    assert!(!tree.lines().any(|line| line == "src/other.rs"));
    let status = git_stdout(dir.path(), &["status", "--porcelain"]);
    assert!(status.lines().any(|line| line == "?? src/new.rs"));
    assert!(status.lines().any(|line| line == "?? src/other.rs"));
}

#[test]
fn rescue_amends_untagged_head() {
    let _permit = test_subprocess::acquire();
    let dir = repo();
    commit_path(dir.path(), "tracked.txt", "tracked");
    let before = head(dir.path());
    let count_before = commit_count(dir.path());
    write_path(dir.path(), "src/amended.rs", "pub fn amended() {}\n");

    let outcome = rescue_dirty_worktree(dir.path().to_str().unwrap(), "task-123").unwrap();

    assert!(outcome.committed);
    assert_ne!(head(dir.path()), before);
    assert_eq!(commit_count(dir.path()), count_before);
}
