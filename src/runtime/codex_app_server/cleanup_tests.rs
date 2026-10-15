// Cleanup regressions for killed workers, live owners, and real-home preservation.
// Only synthetic homes and explicitly owned subprocesses are used.

use super::*;

fn setup() -> (tempfile::TempDir, paths::AidHomeGuard, std::path::PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let guard = paths::AidHomeGuard::set(home.path());
    let task = paths::task_dir("t-cleanup");
    std::fs::create_dir_all(&task).unwrap();
    (home, guard, task)
}

#[test]
fn stale_worker_home_and_marker_are_removed_without_touching_real_entries() {
    let (home, _guard, task) = setup();
    let real = home.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("auth.json"), "synthetic").unwrap();
    std::fs::create_dir(task.join("codex-home")).unwrap();
    std::os::unix::fs::symlink(real.join("auth.json"), task.join("codex-home/auth.json")).unwrap();
    std::fs::write(
        task.join("codex-owner.json"),
        json!({"pid":u32::MAX,"started":"dead"}).to_string(),
    )
    .unwrap();
    std::fs::write(task.join("codex-app-server.json"), "{}").unwrap();
    sweep().unwrap();
    assert!(!task.join("codex-home").exists());
    assert!(!task.join("codex-app-server.json").exists());
    assert!(real.join("auth.json").exists());
}

#[test]
fn live_owner_is_preserved_even_before_a_turn_or_marker_exists() {
    let (_home, _guard, task) = setup();
    let _owner = Owner::create("t-cleanup").unwrap();
    std::fs::create_dir(task.join("codex-home")).unwrap();
    sweep().unwrap();
    assert!(task.join("codex-home").exists());
}

#[test]
fn orphaned_server_is_terminated_before_its_home_is_removed() {
    use std::os::unix::process::CommandExt;
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard, task) = setup();
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exec sleep 60"])
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id();
    let started = activity::root_started(pid).unwrap();
    std::fs::write(
        task.join("codex-app-server.json"),
        json!({"pid":pid,"started":started}).to_string(),
    )
    .unwrap();
    std::fs::write(
        task.join("codex-owner.json"),
        json!({"pid":u32::MAX,"started":"dead"}).to_string(),
    )
    .unwrap();
    std::fs::create_dir(task.join("codex-home")).unwrap();
    sweep().unwrap();
    assert!(!child.wait().unwrap().success());
    assert!(!task.join("codex-home").exists());
}
