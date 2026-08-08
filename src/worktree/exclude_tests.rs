// Tests for keeping aid's runtime files out of the target repo's git status.
// Covers the reported failure: a committed-then-removed .aid-lock reading as agent dirt.
use super::*;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success(),
        "git {args:?} failed"
    );
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "base.txt"]);
    git(dir.path(), &["commit", "-qm", "base"]);
    dir
}

fn status(dir: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn aid_runtime_files_stop_showing_up_as_untracked() {
    let dir = repo();
    std::fs::write(dir.path().join(".aid-lock"), "pid=1234\n").unwrap();
    std::fs::write(dir.path().join(".aid-verify-deps-state"), "{}\n").unwrap();
    std::fs::write(dir.path().join("result-t-abcd1234.md"), "report\n").unwrap();
    assert!(status(dir.path()).contains(".aid-lock"), "precondition: git sees it");

    ensure_aid_paths_excluded(dir.path());

    let after = status(dir.path());
    assert!(after.trim().is_empty(), "aid's own files must not appear: {after}");
}

#[test]
fn a_real_file_is_still_reported() {
    let dir = repo();
    ensure_aid_paths_excluded(dir.path());
    std::fs::write(dir.path().join("delivered.txt"), "work\n").unwrap();
    assert!(
        status(dir.path()).contains("delivered.txt"),
        "the exclude must not swallow the agent's own files"
    );
}

#[test]
fn writing_twice_does_not_duplicate_entries() {
    let dir = repo();
    ensure_aid_paths_excluded(dir.path());
    ensure_aid_paths_excluded(dir.path());
    let content = std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert_eq!(content.matches(".aid-*").count(), 1, "entries must be idempotent: {content}");
    assert_eq!(content.matches(AID_EXCLUDE_HEADER).count(), 1, "header once: {content}");
}

#[test]
fn existing_exclude_content_is_preserved() {
    let dir = repo();
    let exclude = dir.path().join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "# personal\nscratch/\n").unwrap();

    ensure_aid_paths_excluded(dir.path());

    let content = std::fs::read_to_string(&exclude).unwrap();
    assert!(content.contains("scratch/"), "must not clobber what was there: {content}");
    assert!(content.contains(".aid-*"), "and must still add ours: {content}");
}

/// Over-matching here is the dangerous direction: a path wrongly judged aid's own
/// stops counting as uncommitted work and can be discarded.
#[test]
fn only_aid_s_own_paths_are_claimed() {
    use super::super::snapshot::is_aid_owned_path;

    for owned in [
        ".aid-lock",
        ".aid-verify-deps-state",
        "nested/dir/.aid-lock",
        "aid-batch-123.toml",
        ".aid/state.toml",
        ".aid/batches/wg-abc1.toml",
        "result-t-1a2b3c4d.md",
        "result-t-1a2b3c4d.json",
        ".aid-scratch/",
    ] {
        assert!(is_aid_owned_path(owned), "aid writes this: {owned}");
    }

    for theirs in [
        "result-summary.md",
        "results.md",
        "result-2026-q3.json",
        "docs/result-of-the-experiment.md",
        "src/aid_batch_runner.rs",
        ".aidrc",
        "my file.txt",
        "target/debug/thing.so",
        "src/main.rs",
    ] {
        assert!(!is_aid_owned_path(theirs), "this belongs to the user: {theirs}");
    }
}

/// A rename names two paths and porcelain reports only one line for it. Judging the
/// line on its destination alone would let `R  src/lib.rs -> result-t-abcd.md` erase a
/// real file from every dirty check.
#[test]
fn a_rename_is_only_aid_s_when_both_ends_are() {
    let dir = repo();
    std::fs::write(dir.path().join("lib.rs"), "real work\n").unwrap();
    git(dir.path(), &["add", "lib.rs"]);
    git(dir.path(), &["commit", "-qm", "real work"]);
    std::fs::rename(dir.path().join("lib.rs"), dir.path().join("result-t-abcd1234.md")).unwrap();
    git(dir.path(), &["add", "-A", "-f", "."]);

    let snapshot = super::super::capture_worktree_snapshot(dir.path()).unwrap();
    assert!(
        snapshot.has_uncommitted_changes(),
        "a real file renamed onto an aid name is still the agent's change: {:?}",
        snapshot.status_lines
    );
}

/// A filename may itself contain " -> ", which is also porcelain's rename separator.
/// Splitting on it then yields fragments that are not real paths. That has to fail in
/// the safe direction — the line stays dirty — and it does, because git quotes such a
/// name and the fragments keep an unbalanced quote that matches no aid pattern. This
/// is load-bearing and not obvious, so it is pinned here.
#[test]
fn a_filename_containing_the_rename_arrow_still_counts_as_dirty() {
    let dir = repo();
    std::fs::write(dir.path().join("notes -> .aid-lock"), "the user's own file\n").unwrap();

    let snapshot = super::super::capture_worktree_snapshot(dir.path()).unwrap();
    assert!(
        snapshot.has_uncommitted_changes(),
        "a user file whose name contains the rename separator is not aid's: {:?}",
        snapshot.status_lines
    );
}

/// Git wraps names needing escapes in quotes; the quote must not defeat the match.
#[test]
fn a_quoted_aid_path_is_still_recognised() {
    let dir = repo();
    std::fs::write(dir.path().join(".aid-odd name"), "lease\n").unwrap();
    let snapshot = super::super::capture_worktree_snapshot(dir.path()).unwrap();
    assert!(
        !snapshot.has_uncommitted_changes(),
        "aid's file is aid's however git spells it: {:?}",
        snapshot.status_lines
    );
}

/// The reported failure in miniature: an agent commits `.aid-lock`, aid removes it at
/// task end, and the deletion of a tracked file reads as the agent leaving work behind.
/// The exclude does not help once the file is tracked — the dirty gate has to filter it,
/// which is what `agent_status_lines` is for.
#[test]
fn a_deleted_tracked_aid_lock_is_not_agent_dirt() {
    let dir = repo();
    std::fs::write(dir.path().join(".aid-lock"), "pid=1234\n").unwrap();
    git(dir.path(), &["add", "-f", ".aid-lock"]);
    git(dir.path(), &["commit", "-qm", "agent committed aid's lock"]);
    std::fs::remove_file(dir.path().join(".aid-lock")).unwrap();
    assert!(status(dir.path()).contains(".aid-lock"), "precondition: git reports the deletion");

    let snapshot = super::super::capture_worktree_snapshot(dir.path()).unwrap();
    assert!(
        !snapshot.has_uncommitted_changes(),
        "aid removing its own lock is not the agent leaving work: {:?}",
        snapshot.status_lines
    );

    std::fs::write(dir.path().join("delivered.txt"), "work\n").unwrap();
    let snapshot = super::super::capture_worktree_snapshot(dir.path()).unwrap();
    assert!(snapshot.has_uncommitted_changes(), "a real leftover must still count");
}
