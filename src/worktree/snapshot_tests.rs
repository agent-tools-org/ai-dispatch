// Worktree snapshot and staging regression tests.
// Covers status parsing, rescue filtering, and Git bookkeeping exclusions.
// Deps: snapshot helpers, std::process, tempfile.

use super::{
    WorktreeStatusKind, capture_worktree_snapshot, capture_worktree_snapshot_with_base,
    is_rescuable_path, parse_status_entry,
};
use crate::worktree::{AidStageMode, stage_aid_files};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn staged_names(dir: &Path) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .expect("git diff --cached failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn repo_with_main() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "aid@example.com"]);
    git(dir.path(), &["config", "user.name", "Aid Tester"]);
    std::fs::write(dir.path().join("file.txt"), "initial").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn checkout_feature_with_change(dir: &Path) {
    git(dir, &["checkout", "-b", "feature"]);
    std::fs::write(dir.join("file.txt"), "updated").unwrap();
    git(dir, &["add", "file.txt"]);
    git(dir, &["commit", "-m", "feature change"]);
}

#[test]
fn worktree_snapshot_parses_status_entries() {
    let untracked = parse_status_entry("?? src/new.rs").unwrap();
    assert_eq!(untracked.path, "src/new.rs");
    assert_eq!(untracked.kind, WorktreeStatusKind::Untracked);

    let modified = parse_status_entry(" M src/lib.rs").unwrap();
    assert_eq!(modified.path, "src/lib.rs");
    assert_eq!(modified.kind, WorktreeStatusKind::Modified);

    assert!(parse_status_entry(" D src/lib.rs").is_none());
}

#[test]
fn worktree_snapshot_filters_non_source_artifacts() {
    assert!(is_rescuable_path("src/lib.rs"));
    assert!(!is_rescuable_path("target/debug/app"));
    assert!(!is_rescuable_path("cache/file.pyc"));
}

#[test]
fn is_rescuable_path_excludes_aid_artifacts() {
    assert!(!is_rescuable_path("result-t-abc123.md"));
    assert!(!is_rescuable_path("result-t-0d8f.md"));
    assert!(!is_rescuable_path(".aid/results/foo.md"));
    assert!(is_rescuable_path("results/foo.md"));
    assert!(is_rescuable_path("my-result-t.md"));
}

#[test]
fn aid_add_excludes_covers_nested_and_untyped_bookkeeping_paths() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join(".aid")).unwrap();
    std::fs::write(dir.path().join(".aid/seo-phase1.toml"), "tracked = true\n").unwrap();
    git(dir.path(), &["add", "-A", "--", "."]);
    git(dir.path(), &["commit", "-m", "track agent config"]);
    std::fs::write(dir.path().join(".gitignore"), ".aid/\n").unwrap();
    git(dir.path(), &["add", ".gitignore"]);
    git(dir.path(), &["commit", "-m", "ignore aid directory"]);
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::create_dir_all(dir.path().join(".aid/batches")).unwrap();
    std::fs::write(dir.path().join(".aid-lock"), "pid=1\n").unwrap();
    std::fs::write(dir.path().join("sub/.aid-nested"), "pid=1\n").unwrap();
    std::fs::write(dir.path().join(".aid/state.toml"), "health = 1\n").unwrap();
    std::fs::write(dir.path().join(".aid/batches/foo.toml"), "x\n").unwrap();
    std::fs::write(dir.path().join("aid-batch-run.json"), "x\n").unwrap();
    std::fs::write(dir.path().join("sub/aid-batch-nested.log"), "x\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &[]).unwrap();

    let staged = staged_names(dir.path());
    assert_eq!(
        staged.lines().collect::<Vec<_>>(),
        vec!["keep.rs"],
        "got: {staged}"
    );
}

#[test]
fn aid_staging_handles_tracked_state_under_ignored_aid_directory() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join(".aid")).unwrap();
    std::fs::write(dir.path().join(".aid/state.toml"), "initial\n").unwrap();
    git(dir.path(), &["add", ".aid/state.toml"]);
    git(dir.path(), &["commit", "-m", "track aid state"]);
    std::fs::write(dir.path().join(".gitignore"), ".aid/\n").unwrap();
    git(dir.path(), &["add", ".gitignore"]);
    git(dir.path(), &["commit", "-m", "ignore aid directory"]);
    std::fs::write(dir.path().join(".aid/state.toml"), "updated\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &[]).unwrap();

    assert!(staged_names(dir.path()).lines().any(|path| path == "keep.rs"));
}

#[test]
fn aid_add_excludes_handles_state_ignored_by_info_exclude() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join(".aid")).unwrap();
    std::fs::write(
        dir.path().join(".git/info/exclude"),
        ".aid/state.toml\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".aid/state.toml"), "health = 1\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &[]).unwrap();

    let staged = staged_names(dir.path());
    assert_eq!(staged.lines().collect::<Vec<_>>(), vec!["keep.rs"]);
}

#[test]
fn aid_staging_keeps_project_but_excludes_tracked_state() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join(".aid")).unwrap();
    std::fs::write(dir.path().join(".aid/project.toml"), "[project]\nid = \"a\"\n").unwrap();
    std::fs::write(dir.path().join(".aid/state.toml"), "health = 1\n").unwrap();
    git(dir.path(), &["add", "-A", "--", "."]);
    git(dir.path(), &["commit", "-m", "track project.toml"]);
    std::fs::write(dir.path().join(".aid/project.toml"), "[project]\nid = \"b\"\n").unwrap();
    std::fs::write(dir.path().join(".aid/state.toml"), "health = 2\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::Tracked, &[]).unwrap();

    let staged = staged_names(dir.path());
    assert_eq!(staged.lines().collect::<Vec<_>>(), vec![".aid/project.toml"], "got: {staged}");
}

#[test]
fn aid_staging_excludes_populated_ignored_target_directory() {
    let dir = repo_with_main();
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    git(dir.path(), &["add", ".gitignore"]);
    git(dir.path(), &["commit", "-m", "ignore target"]);
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("target/debug/app"), "binary\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &["target/"]).unwrap();

    assert_eq!(staged_names(dir.path()).lines().collect::<Vec<_>>(), vec!["keep.rs"]);
}

#[test]
fn aid_staging_excludes_target_directory_even_when_not_ignored() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("target/debug/app"), "binary\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &["target/"]).unwrap();

    assert_eq!(staged_names(dir.path()).lines().collect::<Vec<_>>(), vec!["keep.rs"]);
}

#[test]
fn aid_staging_excludes_untracked_state_without_ignore_rules() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join(".aid/batches")).unwrap();
    std::fs::write(dir.path().join(".aid/state.toml"), "health = 1\n").unwrap();
    std::fs::write(dir.path().join(".aid/batches/run.toml"), "batch = 1\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &[]).unwrap();

    assert_eq!(staged_names(dir.path()).lines().collect::<Vec<_>>(), vec!["keep.rs"]);
}

#[test]
fn aid_staging_preserves_pre_staged_target_file_when_target_is_ignored() {
    let dir = repo_with_main();
    std::fs::create_dir_all(dir.path().join("target")).unwrap();
    std::fs::write(dir.path().join("target/artifact"), "initial\n").unwrap();
    git(dir.path(), &["add", "-A", "--", "."]);
    git(dir.path(), &["commit", "-m", "track target artifact"]);
    std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    git(dir.path(), &["add", ".gitignore"]);
    git(dir.path(), &["commit", "-m", "ignore target"]);
    std::fs::write(dir.path().join("target/artifact"), "agent update\n").unwrap();
    git(dir.path(), &["add", "-f", "target/artifact"]);
    std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();

    stage_aid_files(dir.path(), AidStageMode::All, &["target/"]).unwrap();

    assert_eq!(
        staged_names(dir.path()).lines().collect::<Vec<_>>(),
        vec!["keep.rs", "target/artifact"]
    );
}

#[test]
fn empty_diff_is_false_for_committed_only_change_against_base() {
    let dir = repo_with_main();
    checkout_feature_with_change(dir.path());

    let snapshot = capture_worktree_snapshot_with_base(dir.path(), Some("main")).unwrap();

    assert_eq!(snapshot.empty_diff, Some(false));
    assert!(!snapshot.has_uncommitted_changes());
}

#[test]
fn empty_diff_is_true_for_clean_worktree_without_commits_ahead() {
    let dir = repo_with_main();

    let snapshot = capture_worktree_snapshot_with_base(dir.path(), Some("main")).unwrap();

    assert_eq!(snapshot.empty_diff, Some(true));
}

#[test]
fn empty_diff_is_false_for_dirty_uncommitted_change() {
    let dir = repo_with_main();
    std::fs::write(dir.path().join("file.txt"), "dirty").unwrap();

    let snapshot = capture_worktree_snapshot_with_base(dir.path(), Some("main")).unwrap();

    assert_eq!(snapshot.empty_diff, Some(false));
    assert!(snapshot.has_uncommitted_changes());
}

#[test]
fn empty_diff_uses_default_branch_fallback_for_committed_change() {
    let dir = repo_with_main();
    checkout_feature_with_change(dir.path());

    let snapshot = capture_worktree_snapshot(dir.path()).unwrap();

    assert_eq!(snapshot.empty_diff, Some(false));
}
