// Verify failure and pending-state regressions moved from the gate suite.
// Deps: parent gate fixture, Store and verification helpers.

use super::*;

#[test]
fn genuine_verify_failure_still_fails_task() {
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-real-vfail".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("false")))
        .unwrap();

    maybe_verify(&store, &task_id, Some("false"), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::Failed);
    assert_eq!(loaded.status, TaskStatus::Failed);
}

#[cfg(unix)]
#[test]
fn configured_verify_records_pending_before_command_finishes() {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-verify-pending-start".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done, Some(&dir_str), Some("sleep 1")))
        .unwrap();

    let verify_store = store.clone();
    let verify_id = task_id.clone();
    let handle = std::thread::spawn(move || {
        maybe_verify(&verify_store, &verify_id, Some("sleep 1"), Some(&dir_str), None);
    });

    let mut observed_pending = false;
    for _ in 0..50 {
        if store
            .get_task(task_id.as_str())
            .unwrap()
            .is_some_and(|loaded| loaded.verify_status == VerifyStatus::Pending)
        {
            observed_pending = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(observed_pending);
    handle.join().unwrap();
    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().verify_status,
        VerifyStatus::Passed
    );
}
