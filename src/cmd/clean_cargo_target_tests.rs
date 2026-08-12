// Tests for task-owned Cargo target and fallback cleanup.
// Covers terminal ownership, live-worktree protection, and unattributed paths.
// Deps: clean_cargo_target, Store, tempfile, rusqlite.

use super::*;
use crate::test_env::CargoTargetDirGuard;
use crate::store::Store;
use rusqlite::params;
use std::fs;
use std::path::Path;

fn insert_task(
    store: &Store,
    id: &str,
    status: &str,
    repo_path: Option<&Path>,
    worktree_path: Option<&Path>,
    branch: Option<&str>,
) {
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, worktree_branch, created_at)
             VALUES (?1, 'codex', 'cleanup test', ?2, ?3, ?4, ?5, '2026-01-01T00:00:00Z')",
            params![
                id,
                status,
                repo_path.map(|path| path.to_string_lossy().into_owned()),
                worktree_path.map(|path| path.to_string_lossy().into_owned()),
                branch,
            ],
        )
        .unwrap();
}

fn init_repo(repo: &Path) {
    std::process::Command::new("git")
        .args(["init", "-b", "main", &repo.to_string_lossy()])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "config", "user.name", "Test User"])
        .status()
        .unwrap();
    fs::write(repo.join("file.txt"), "base\n").unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "add", "file.txt"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "commit", "-m", "base"])
        .status()
        .unwrap();
}

#[test]
fn cleanup_removes_only_terminal_task_owned_targets() {
    let _permit = crate::test_subprocess::acquire();
    let aid_home = tempfile::tempdir().unwrap();
    let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let target_root = aid_home.path().join("cargo-target");
    let _target_guard = CargoTargetDirGuard::set(&target_root);
    let fallback_root = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();

    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let live_worktree = tempfile::tempdir().unwrap();
    let live_branch = "feat/live-clean-target";
    let live_path = live_worktree.path().join("live");
    std::process::Command::new("git")
        .args([
            "-C",
            &repo.path().to_string_lossy(),
            "worktree",
            "add",
            &live_path.to_string_lossy(),
            "-b",
            live_branch,
        ])
        .status()
        .unwrap();

    let stale_branch = "feat/stale-clean-target";
    let stale_worktree = repo.path().join("missing-worktree");
    insert_task(
        &store,
        "t-stale",
        "done",
        Some(repo.path()),
        Some(&stale_worktree),
        Some(stale_branch),
    );
    insert_task(
        &store,
        "t-live",
        "done",
        Some(repo.path()),
        Some(&live_path),
        Some(live_branch),
    );
    insert_task(&store, "t-blank-branch", "done", None, None, Some(""));

    let stale_target = target_root.join(crate::agent::env::branch_target_name(stale_branch));
    let live_target = target_root.join(crate::agent::env::branch_target_name(live_branch));
    let stale_fallback = fallback_root
        .path()
        .join(crate::cmd::build::build_fallback::cwd_key(&stale_worktree));
    let unattributed = target_root.join("not-in-task-record");
    for path in [&stale_target, &live_target, &stale_fallback, &unattributed] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("artifact"), vec![b'x'; 8]).unwrap();
    }
    fs::write(target_root.join("root-artifact"), b"root").unwrap();

    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    clean_orphaned_branch_targets(&store, false, Some(fallback_root.path()), &mut sizes).unwrap();

    assert!(!stale_target.exists());
    assert!(!stale_fallback.exists());
    assert!(live_target.exists());
    assert!(unattributed.exists());
    assert!(target_root.exists());
}
