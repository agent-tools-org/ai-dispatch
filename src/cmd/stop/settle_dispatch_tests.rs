// Dispatch races at the SQLite write-lock boundary after a stale status read.
// Checks waiting replacement and launch cannot revive a stopped task.
// Deps: two Store connections, SQLite busy callback and bounded channels.

use super::tests::make_task;
use crate::{store::Store, types::TaskStatus};
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

thread_local! {
    static WRITE_BARRIER: RefCell<Option<(Sender<()>, Receiver<()>)>> = const { RefCell::new(None) };
}

fn busy_after_read(_: i32) -> bool {
    WRITE_BARRIER.with(|barrier| {
        if let Some((ready, release)) = barrier.borrow_mut().take() {
            ready.send(()).expect("write blocked");
            release.recv_timeout(Duration::from_secs(10)).expect("release writer");
        }
    });
    true
}

fn race_stopped_write(replace: bool) {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    let path = home.path().join("race.db");
    let writer = Store::open(&path).expect("writer");
    let dispatch = Store::open(&path).expect("dispatch");
    let status = if replace { TaskStatus::Waiting } else { TaskStatus::Pending };
    writer.insert_task(&make_task("t-stop-dispatch", status)).expect("insert");
    writer.db().execute_batch("BEGIN IMMEDIATE").expect("hold writer lock");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatch_home = home.path().to_path_buf();
    let handle = std::thread::spawn(move || {
        let _guard = crate::paths::AidHomeGuard::set(&dispatch_home);
        WRITE_BARRIER.with(|barrier| *barrier.borrow_mut() = Some((ready_tx, release_rx)));
        dispatch.db().busy_handler(Some(busy_after_read)).expect("busy callback");
        if replace {
            dispatch.replace_waiting_task(&make_task("t-stop-dispatch", TaskStatus::Pending)).is_ok()
        } else {
            dispatch.update_task_status("t-stop-dispatch", TaskStatus::Running).expect("launch")
        }
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).expect("dispatch read before stop");
    writer.update_task_status("t-stop-dispatch", TaskStatus::Stopped).expect("settle stop");
    writer.db().execute_batch("COMMIT").expect("commit stop");
    release_tx.send(()).expect("release dispatch");
    assert!(!handle.join().expect("dispatch"), "stale dispatch must be refused");
    assert_eq!(writer.get_task("t-stop-dispatch").expect("get").expect("task").status,
        TaskStatus::Stopped);
    assert!(writer.get_events("t-stop-dispatch").expect("events").is_empty());
}

#[test]
fn settle_first_waiting_replacement_cannot_revive_concurrent_stop() {
    race_stopped_write(true);
}

#[test]
fn settle_first_launch_cannot_revive_concurrent_stop() {
    race_stopped_write(false);
}
