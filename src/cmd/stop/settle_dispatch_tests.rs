// Dispatch races at the SQLite write-lock boundary after a stale status read.
// Checks waiting replacement and launch cannot revive a stopped task.
// Deps: two Store connections, SQLite busy callback and bounded channels.

use super::{stop, tests::make_task};
use crate::{store::Store, types::TaskStatus};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

thread_local! {
    static WRITE_BARRIER: RefCell<Option<(Sender<()>, Receiver<()>)>> = const { RefCell::new(None) };
}

fn busy_after_read(_: i32) -> bool {
    WRITE_BARRIER.with(|barrier| {
        if let Some((ready, release)) = barrier.borrow_mut().take() {
            ready.send(()).expect("write blocked");
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("release writer");
        }
    });
    true
}

fn race_stopped_write(replace: bool) {
    let (home, _guard, writer, dispatch) = setup_race(replace);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatch_home = home.path().to_path_buf();
    let handle = std::thread::spawn(move || {
        let _guard = crate::support::paths::AidHomeGuard::set(&dispatch_home);
        WRITE_BARRIER.with(|barrier| *barrier.borrow_mut() = Some((ready_tx, release_rx)));
        dispatch
            .db()
            .busy_handler(Some(busy_after_read))
            .expect("busy callback");
        if replace {
            dispatch
                .replace_waiting_task(&make_task("t-stop-dispatch", TaskStatus::Pending))
                .is_ok()
        } else {
            dispatch
                .update_task_status("t-stop-dispatch", TaskStatus::Running)
                .expect("launch")
        }
    });
    ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("dispatch read before stop");
    writer
        .db()
        .execute_batch("ROLLBACK")
        .expect("release blocker lock");
    stop(&writer, "t-stop-dispatch").expect("settle stop before dispatch resumes");
    release_tx.send(()).expect("release dispatch");
    assert!(
        !handle.join().expect("dispatch"),
        "stale dispatch must be refused"
    );
    assert_eq!(
        writer
            .get_task("t-stop-dispatch")
            .expect("get")
            .expect("task")
            .status,
        TaskStatus::Stopped
    );
    let events = writer.get_events("t-stop-dispatch").expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, "Task stopped by user");
}

fn setup_race(replace: bool) -> (tempfile::TempDir, crate::support::paths::AidHomeGuard, Arc<Store>, Store) {
    let home = tempfile::tempdir().expect("home");
    let guard = crate::support::paths::AidHomeGuard::set(home.path());
    let path = home.path().join("race.db");
    let writer = Arc::new(Store::open(&path).expect("writer"));
    let dispatch = Store::open(&path).expect("dispatch");
    let status = if replace {
        TaskStatus::Waiting
    } else {
        TaskStatus::Pending
    };
    writer
        .insert_task(&make_task("t-stop-dispatch", status))
        .expect("insert");
    writer
        .db()
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold writer lock");
    (home, guard, writer, dispatch)
}

#[test]
fn settle_first_waiting_replacement_cannot_revive_concurrent_stop() {
    race_stopped_write(true);
}

#[test]
fn settle_first_launch_cannot_revive_concurrent_stop() {
    race_stopped_write(false);
}

#[test]
fn settle_first_stopped_launch_claim_is_silent() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    let task = make_task("t-stopped-claim", TaskStatus::Pending);
    store.insert_task(&task).expect("task");
    stop(&store, task.id.as_str()).expect("stop");
    assert!(!crate::task::lifecycle::mark_running(&store, &task.id).expect("launch claim"));
    let saved = store.get_task(task.id.as_str()).expect("get").expect("task");
    assert_eq!(saved.status, TaskStatus::Stopped);
    assert_eq!(store.get_events(task.id.as_str()).expect("events").len(), 1);
}
