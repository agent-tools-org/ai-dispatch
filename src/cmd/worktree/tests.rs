// Tests for `aid worktree` list/prune lock behavior.
// Covers live-lock pruning protection, dead-lock cleanup, and JSON listing shape.
// Deps: super command helpers, git CLI, tempfile.

use super::{list_json, prune, should_prune_worktree};
use crate::test_subprocess;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo(repo_dir: &Path) {
    git(repo_dir, &["init", "-b", "main"]);
    git(repo_dir, &["config", "user.email", "test@example.com"]);
    git(repo_dir, &["config", "user.name", "Test User"]);
    std::fs::write(repo_dir.join("file.txt"), "hello\n").unwrap();
    git(repo_dir, &["add", "file.txt"]);
    git(repo_dir, &["commit", "-m", "init"]);
}

fn legacy_path(name: &str) -> PathBuf {
    Path::new("/tmp").join(format!("aid-wt-{}-{name}", std::process::id()))
}

fn add_worktree(repo_dir: &Path, branch: &str, name: &str) -> PathBuf {
    let path = legacy_path(name);
    let _ = std::fs::remove_dir_all(&path);
    git(
        repo_dir,
        &[
            "worktree",
            "add",
            &path.to_string_lossy(),
            "-b",
            branch,
        ],
    );
    path
}

fn remove_worktree(repo_dir: &Path, path: &Path) {
    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "remove", "--force", &path.to_string_lossy()])
        .status();
    let _ = std::fs::remove_dir_all(path);
}

fn make_old(path: &Path) {
    let status = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn entry_for_path<'a>(entries: &'a [Value], path: &Path) -> &'a Value {
    let path = path.to_string_lossy();
    entries
        .iter()
        .find(|entry| entry.get("path").and_then(Value::as_str) == Some(path.as_ref()))
        .unwrap()
}

#[test]
fn should_prune_worktree_old_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("aid-wt-old");
    std::fs::create_dir(&path).unwrap();
    make_old(&path);
    assert!(should_prune_worktree(path.to_str().unwrap()));
}

#[test]
fn should_prune_worktree_recent_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("aid-wt-recent");
    std::fs::create_dir(&path).unwrap();
    assert!(!should_prune_worktree(path.to_str().unwrap()));
}

#[test]
fn prune_skips_worktree_with_live_lock() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let wt = add_worktree(repo.path(), "feat/live-lock", "live-lock");
    std::fs::write(
        wt.join(".aid-lock"),
        format!("pid={}\ntask=t-live\n", std::process::id()),
    )
    .unwrap();
    make_old(&wt);

    assert!(!should_prune_worktree(wt.to_str().unwrap()));
    prune(Some(repo.path().to_str().unwrap())).unwrap();
    assert!(wt.exists());
    assert!(wt.join(".aid-lock").exists());

    remove_worktree(repo.path(), &wt);
}

#[test]
fn prune_clears_dead_lock_and_removes_old_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let wt = add_worktree(repo.path(), "feat/dead-lock", "dead-lock");
    std::fs::write(wt.join(".aid-lock"), "pid=999999999\ntask=t-dead\n").unwrap();
    make_old(&wt);

    assert!(should_prune_worktree(wt.to_str().unwrap()));
    prune(Some(repo.path().to_str().unwrap())).unwrap();
    assert!(!wt.exists());
    assert!(!wt.join(".aid-lock").exists());
}

#[test]
fn list_json_reports_active_and_inactive_worktrees() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let active = add_worktree(repo.path(), "feat/json-active", "json-active");
    let inactive = add_worktree(repo.path(), "feat/json-inactive", "json-inactive");
    let dead_locked = add_worktree(repo.path(), "feat/json-dead", "json-dead");
    std::fs::write(
        active.join(".aid-lock"),
        format!("pid={}\ntask=t-json\n", std::process::id()),
    )
    .unwrap();
    std::fs::write(dead_locked.join(".aid-lock"), "pid=999999999\ntask=t-dead\n").unwrap();

    let json = list_json(Some(repo.path().to_str().unwrap()), false).unwrap();
    let entries = serde_json::from_str::<Vec<Value>>(&json).unwrap();
    let active_entry = entry_for_path(&entries, &active);
    let inactive_entry = entry_for_path(&entries, &inactive);
    let dead_entry = entry_for_path(&entries, &dead_locked);

    assert_eq!(active_entry.get("branch").and_then(Value::as_str), Some("feat/json-active"));
    assert_eq!(active_entry.get("active").and_then(Value::as_bool), Some(true));
    assert_eq!(active_entry.get("lock_pid").and_then(Value::as_u64), Some(std::process::id() as u64));
    assert_eq!(active_entry.get("lock_task_id").and_then(Value::as_str), Some("t-json"));
    assert!(active_entry.get("modified_age_secs").and_then(Value::as_u64).is_some());
    assert_eq!(inactive_entry.get("active").and_then(Value::as_bool), Some(false));
    assert!(inactive_entry.get("lock_pid").is_some_and(Value::is_null));
    assert!(inactive_entry.get("lock_task_id").is_some_and(Value::is_null));
    assert_eq!(dead_entry.get("active").and_then(Value::as_bool), Some(false));
    assert!(dead_entry.get("lock_pid").is_some_and(Value::is_null));
    assert!(dead_entry.get("lock_task_id").is_some_and(Value::is_null));

    let active_json = list_json(Some(repo.path().to_str().unwrap()), true).unwrap();
    let active_only = serde_json::from_str::<Vec<Value>>(&active_json).unwrap();
    assert_eq!(active_only.len(), 1);
    assert_eq!(
        active_only[0].get("path").and_then(Value::as_str),
        Some(active.to_string_lossy().as_ref())
    );

    remove_worktree(repo.path(), &active);
    remove_worktree(repo.path(), &inactive);
    remove_worktree(repo.path(), &dead_locked);
}
