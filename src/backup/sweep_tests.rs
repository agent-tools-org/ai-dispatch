// Backup sweep: the one-hour window and 30-second quiet bounds, the per-tick
// attempt cap, in-flight workers, the once-guard shared with on_settled, one
// marker for a config error, and a reaper-failed task bundled with its failure.

use super::lifecycle_tests::{failing_gws, save_args, task};
use super::sweep::sweep_at;
use super::*;
use crate::cmd::run::RunArgs;
use crate::paths::AidHomeGuard;
use crate::types::TaskStatus;
use chrono::{DateTime, Duration, Local};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn gdrive_args() -> RunArgs {
    RunArgs { backup: Some("gdrive:audits".into()), ..Default::default() }
}

fn ended_task(store: &Store, id: &str, status: TaskStatus, completed_at: DateTime<Local>, args: RunArgs) {
    let mut ended = task(id, status);
    ended.completed_at = Some(completed_at);
    store.insert_task(&ended).unwrap();
    save_args(store, id, args);
}

fn markers(store: &Store, id: &str) -> Vec<TaskEvent> {
    let events = store.get_events(id).unwrap();
    events.into_iter().filter(|e| e.metadata.as_ref().is_some_and(|m| m.get("backup").is_some())).collect()
}

fn idle(_: &str) -> bool {
    false
}

fn calls(log: &Path) -> usize {
    fs::read_to_string(log).map(|calls| calls.lines().count()).unwrap_or(0)
}

#[test]
fn sweep_attempts_only_tasks_that_ended_inside_the_window() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = failing_gws(home.path());
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    ended_task(&store, "t-win", TaskStatus::Done, now, gdrive_args());
    // Completed three hours ago; a later event (e.g. an accept) must not
    // pull history back into the window.
    ended_task(&store, "t-history", TaskStatus::Done, now - Duration::hours(3), gdrive_args());
    store.insert_event(&TaskEvent {
        task_id: TaskId("t-history".into()),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail: "accepted".into(),
        metadata: None,
    }).unwrap();

    assert_eq!(sweep_at(&store, now + Duration::seconds(10), idle), 0, "too fresh");
    assert_eq!(sweep_at(&store, now + Duration::hours(2), idle), 0, "too old");
    assert_eq!(calls(&log), 0);
    assert_eq!(sweep_at(&store, now + Duration::minutes(5), idle), 1, "in window");

    assert_eq!(markers(&store, "t-win").len(), 1);
    assert!(markers(&store, "t-history").is_empty());
    assert_eq!(calls(&log), 1);
}

#[test]
fn sweep_caps_attempts_per_tick_and_skips_unconfigured_tasks() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = failing_gws(home.path());
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    for i in 0..3 {
        ended_task(&store, &format!("t-plain-{i}"), TaskStatus::Done, now - Duration::seconds(1), RunArgs::default());
    }
    for i in 0..7 {
        ended_task(&store, &format!("t-cap-{i}"), TaskStatus::Failed, now, gdrive_args());
    }
    let tick = now + Duration::minutes(5);

    assert_eq!(sweep_at(&store, tick, idle), 5);
    assert_eq!(sweep_at(&store, tick, idle), 2);
    assert_eq!(sweep_at(&store, tick, idle), 0);

    assert_eq!(calls(&log), 7);
    for i in 0..7 {
        assert_eq!(markers(&store, &format!("t-cap-{i}")).len(), 1);
    }
}

#[test]
fn sweep_leaves_a_task_with_a_live_worker_to_its_post_run_lifecycle() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = failing_gws(home.path());
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    ended_task(&store, "t-verifying", TaskStatus::Done, now, gdrive_args());
    let tick = now + Duration::minutes(5);

    assert_eq!(sweep_at(&store, tick, |id| id == "t-verifying"), 0);
    assert_eq!(calls(&log), 0);
    assert!(!super::sweep::worker_alive("t-verifying"), "no job spec, no worker");
    assert_eq!(sweep_at(&store, tick, idle), 1, "worker gone: the sweep takes it");
}

#[test]
fn sweep_never_reattempts_a_task_settled_by_the_post_run_lifecycle() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let log = failing_gws(home.path());
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    ended_task(&store, "t-settled", TaskStatus::Done, now, gdrive_args());

    on_settled(&store, "t-settled");
    assert_eq!(sweep_at(&store, now + Duration::minutes(5), idle), 0);

    assert_eq!(calls(&log), 1);
    assert_eq!(markers(&store, "t-settled").len(), 1);
}

#[test]
fn config_error_records_exactly_one_marker_across_sweeps() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join(".aid")).unwrap();
    fs::write(
        repo.path().join(".aid/project.toml"),
        "[project]\nid = 'proj'\n[backup]\ntarget = 'gdrive'\ninclude = ['bogus']\n",
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    let mut bad = task("t-bad-config", TaskStatus::Done);
    bad.completed_at = Some(now);
    bad.repo_path = Some(repo.path().display().to_string());
    store.insert_task(&bad).unwrap();
    save_args(&store, "t-bad-config", RunArgs::default());

    assert_eq!(sweep_at(&store, now + Duration::minutes(5), idle), 1);
    assert_eq!(sweep_at(&store, now + Duration::minutes(6), idle), 0);
    on_settled(&store, "t-bad-config");

    let found = markers(&store, "t-bad-config");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].detail.contains("unknown backup include 'bogus'"), "{}", found[0].detail);
}

/// A fake `gws` that copies any uploaded file into `capture` and succeeds.
fn capturing_gws(home: &Path, capture: &Path) -> PathBuf {
    let binary = home.join("gws");
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do last=$a; done\n\
         [ -f \"$last\" ] && cp \"$last\" '{}'/\necho '{{\"id\":\"fake123\"}}'\n",
        capture.display()
    );
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(crate::paths::config_path(), format!("[backup.gdrive]\nbinary = '{}'\n", binary.display())).unwrap();
    binary
}

#[test]
fn reaper_failed_task_is_swept_with_its_failure_event() {
    let _permit = crate::test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let capture = home.path().join("capture");
    fs::create_dir_all(&capture).unwrap();
    capturing_gws(home.path(), &capture);
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    store.insert_task(&task("t-reaped", TaskStatus::Running)).unwrap();
    save_args(&store, "t-reaped", gdrive_args());

    // The reaper's order: fail the execution, then persist its error event.
    assert!(crate::task_lifecycle::fail_active_execution(&store, "t-reaped").unwrap());
    store.insert_event(&TaskEvent {
        task_id: TaskId("t-reaped".into()),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: "Background worker died unexpectedly".into(),
        metadata: None,
    }).unwrap();
    assert_eq!(sweep_at(&store, now + Duration::seconds(5), idle), 0, "reaper path still persisting");
    assert_eq!(sweep_at(&store, now + Duration::minutes(2), idle), 1);

    assert_eq!(markers(&store, "t-reaped")[0].metadata.as_ref().unwrap()["backup"], "uploaded");
    let bundle = fs::read_dir(&capture).unwrap().next().unwrap().unwrap().path();
    let export = Command::new("tar").arg("-xzOf").arg(&bundle).arg("./export.md").output().unwrap();
    let export = String::from_utf8_lossy(&export.stdout);
    assert!(export.contains("Status: failed"), "{export}");
    assert!(export.contains("Background worker died unexpectedly"), "{export}");
}
