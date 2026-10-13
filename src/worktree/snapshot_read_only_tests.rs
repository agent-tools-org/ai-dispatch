// Read-only snapshot regressions for repository-wide content and Git state.
// Exercises the snapshot boundary with real Git, unborn repositories and FIFOs.
// Deps: parent capture helper, std filesystem/process, tempfile.

use super::capture;
use std::{fs, path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn repo() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    git(temp.path(), &["config", "user.name", "Test"]);
    fs::write(temp.path().join("file.txt"), "base\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "base"]);
    temp
}

#[test]
fn read_only_snapshot_from_subdirectory_detects_sibling_changes() {
    let repo = repo();
    let sub = repo.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let before = capture(&sub).unwrap();
    fs::write(repo.path().join("file.txt"), "changed\n").unwrap();
    assert_ne!(before, capture(&sub).unwrap());
    assert_eq!(capture(repo.path()).unwrap(), capture(&sub).unwrap());
}

#[test]
fn read_only_snapshot_detects_commit_of_already_staged_changes() {
    let repo = repo();
    fs::write(repo.path().join("file.txt"), "staged\n").unwrap();
    git(repo.path(), &["add", "."]);
    let before = capture(repo.path()).unwrap();
    git(repo.path(), &["commit", "-m", "staged"]);
    assert_ne!(before, capture(repo.path()).unwrap());
}

#[test]
fn read_only_snapshot_detects_ref_creation_deletion_and_retargeting() {
    let repo = repo();
    let before = capture(repo.path()).unwrap();
    git(repo.path(), &["tag", "audit"]);
    let tagged = capture(repo.path()).unwrap();
    assert_ne!(before, tagged);
    git(repo.path(), &["tag", "-d", "audit"]);
    assert_eq!(before, capture(repo.path()).unwrap());
    git(repo.path(), &["commit", "--allow-empty", "-m", "empty"]);
    git(repo.path(), &["branch", "audit", "HEAD~1"]);
    let branch = capture(repo.path()).unwrap();
    git(repo.path(), &["branch", "-f", "audit", "HEAD"]);
    assert_ne!(branch, capture(repo.path()).unwrap());
}

#[test]
fn read_only_snapshot_detects_detaching_head_without_content_changes() {
    let repo = repo();
    let before = capture(repo.path()).unwrap();
    git(repo.path(), &["checkout", "--detach"]);
    assert_ne!(before, capture(repo.path()).unwrap());
}

#[test]
fn read_only_snapshot_records_unborn_embedded_repository() {
    let repo = repo();
    let nested = repo.path().join("nested");
    fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-b", "main"]);
    let state = capture(repo.path()).unwrap();
    assert_eq!(state, capture(repo.path()).unwrap());
    assert!(state.iter().any(|entry| entry.contains("unborn")));
}

#[test]
fn read_only_snapshot_records_repository_without_commits() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    let before = capture(repo.path()).unwrap();
    assert!(before.iter().any(|entry| entry.contains("unborn")));
    assert_eq!(before, capture(repo.path()).unwrap());
    fs::write(repo.path().join("note.txt"), "new\n").unwrap();
    assert_ne!(before, capture(repo.path()).unwrap());
}

#[test]
fn read_only_snapshot_records_special_files_without_opening_them() {
    let repo = repo();
    let fifo = repo.path().join("file.txt");
    fs::remove_file(&fifo).unwrap();
    assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let before = capture(repo.path()).unwrap();
    assert_eq!(before, capture(repo.path()).unwrap());
    fs::remove_file(&fifo).unwrap();
    fs::write(&fifo, "base\n").unwrap();
    assert_ne!(before, capture(repo.path()).unwrap());
}
