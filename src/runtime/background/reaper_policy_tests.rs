// reaper policy tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn marks_stale_waiting_tasks_failed() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    let mut orphan = make_task("t-wait", TaskStatus::Waiting);
    orphan.created_at = Local::now() - Duration::minutes(61);
    store.insert_task(&orphan).unwrap();

    let mut orphaned_group = make_task("t-wait-group", TaskStatus::Waiting);
    orphaned_group.workgroup_id = Some("wg-orphan".to_string());
    orphaned_group.created_at = Local::now() - Duration::minutes(61);
    store.insert_task(&orphaned_group).unwrap();

    let mut blocked = make_task("t-wait-blocked", TaskStatus::Waiting);
    blocked.workgroup_id = Some("wg-active".to_string());
    blocked.created_at = Local::now() - Duration::minutes(61);
    store.insert_task(&blocked).unwrap();

    let mut blocker = make_task("t-run-blocker", TaskStatus::Running);
    blocker.workgroup_id = Some("wg-active".to_string());
    store.insert_task(&blocker).unwrap();

    let cleaned = waiting::cleanup_stale_waiting_tasks(&store, 60).unwrap();

    assert_eq!(cleaned.len(), 2);
    assert!(cleaned.contains(&"t-wait".to_string()));
    assert!(cleaned.contains(&"t-wait-group".to_string()));

    let failed = store.get_task("t-wait").unwrap().unwrap();
    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(
        failed.pending_reason.as_deref(),
        Some(PendingReason::WaitTimeout.as_str())
    );
    let orphaned_group_failed = store.get_task("t-wait-group").unwrap().unwrap();
    assert_eq!(orphaned_group_failed.status, TaskStatus::Failed);
    assert_eq!(
        orphaned_group_failed.pending_reason.as_deref(),
        Some(PendingReason::WaitTimeout.as_str())
    );
    let blocked_task = store.get_task("t-wait-blocked").unwrap().unwrap();
    assert_eq!(blocked_task.status, TaskStatus::Waiting);
    assert_eq!(blocked_task.pending_reason, None);
    assert!(store.get_events("t-wait-blocked").unwrap().is_empty());

    let error = store.latest_error("t-wait").unwrap();
    assert!(error.contains("wait timeout: no agent slot available"));
    let group_error = store.latest_error("t-wait-group").unwrap();
    assert!(group_error.contains("wait timeout: no agent slot available"));
}

#[test]
fn check_zombie_tasks_auto_fails_old_running_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-a24f", TaskStatus::Running);
    task.created_at = Local::now() - Duration::hours(25);
    store.insert_task(&task).unwrap();

    let cleaned = check_zombie_tasks_with(&store, |_| true).unwrap();

    assert_eq!(cleaned, vec!["t-a24f".to_string()]);
    assert_eq!(
        store.get_task("t-a24f").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-a24f").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
    assert_eq!(events[0].detail, "Task exceeded maximum runtime (24h)");

    let stderr = std::fs::read_to_string(paths::stderr_path("t-a24f")).unwrap();
    assert_eq!(stderr.trim(), "Auto-failed: exceeded 24h maximum runtime");
}

#[test]
fn check_worker_capacity_warns_at_soft_limit() {
    let store = Store::open_memory().unwrap();
    // No tasks running — should pass silently
    assert!(super::check_worker_capacity(&store).is_ok());
}

#[test]
fn check_worker_capacity_rejects_at_hard_limit() {
    let store = Store::open_memory().unwrap();
    // Insert MAX_WORKERS running tasks to trigger hard limit
    for i in 0..super::MAX_WORKERS {
        let id = format!("t-cap{i:03}");
        store
            .insert_task(&make_task(&id, TaskStatus::Running))
            .unwrap();
    }
    let result = super::check_worker_capacity(&store);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Worker limit reached")
    );
}

#[cfg(unix)]
#[test]
fn is_process_running_returns_false_for_zombie() {
    let _permit = test_subprocess::acquire();
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            libc::_exit(0);
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
        let child_pid = pid as u32;
        let Ok(ps_output) = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &child_pid.to_string()])
            .output()
        else {
            let mut status: i32 = 0;
            libc::waitpid(pid, &mut status, 0);
            return;
        };
        if !ps_output.status.success() || ps_output.stdout.is_empty() {
            let mut status: i32 = 0;
            libc::waitpid(pid, &mut status, 0);
            return;
        }

        assert!(!super::is_process_running(child_pid));

        let mut status: i32 = 0;
        libc::waitpid(pid, &mut status, 0);
    }
}

#[test]
fn zombie_cleanup_skips_autocommit_for_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-a1b2", TaskStatus::Running);
    task.read_only = true;
    store.insert_task(&task).unwrap();

    save_spec(&BackgroundRunSpec {
        worker_pid: Some(999999),
        ..make_spec("t-a1b2")
    })
    .unwrap();

    let cleaned = check_zombie_tasks_with(&store, |_| false).unwrap();
    assert_eq!(cleaned, vec!["t-a1b2".to_string()]);

    let failed_task = store.get_task("t-a1b2").unwrap().unwrap();
    assert_eq!(failed_task.status, TaskStatus::Failed);
}

#[test]
fn reaper_kills_processes_even_when_row_is_already_terminal() {
    // Race regression: a task the reaper snapshotted can be terminalized by a
    // concurrent path (e.g. `aid stop`) before record_failure runs. The DB
    // write is then a no-op, but the processes must still be killed.
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-race", TaskStatus::Running);
    task.status = TaskStatus::Stopped; // already terminal via concurrent stop
    store.insert_task(&task).unwrap();
    let spec = BackgroundRunSpec {
        worker_pid: Some(303),
        agent_pid: Some(404),
        agent_started: None,
        ..make_spec("t-race")
    };
    save_spec(&spec).unwrap();

    kill::RECORDED_KILLS.with(|kills| kills.borrow_mut().clear());
    let mut cleaned = Vec::new();
    reaper::cleanup_pid_task(
        &store,
        &store.get_task("t-race").unwrap().unwrap(),
        &spec,
        303,
        &mut cleaned,
        &|_pid| false, // dead worker => zombie reap condition established
        60,
    )
    .unwrap();

    assert!(
        cleaned.is_empty(),
        "no-op transition must not be reported as cleaned"
    );
    let kills = kill::RECORDED_KILLS.with(|kills| kills.borrow().clone());
    assert_eq!(
        kills,
        vec![vec![303, 404]],
        "processes must be killed despite terminal row"
    );
}
