// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn stop_followup_child_launched_before_parent_stop_is_unaffected() {
    let (_home, _guard, store, parent, _) = setup();
    let mut child = fixture::task("t-child", TaskStatus::Running, None, None);
    child.parent_task_id = Some(parent.id.to_string());
    store.insert_task(&child).unwrap();
    let mut launched = false;
    store
        .with_worker_launch(child.id.as_str(), || {
            launched = true;
            Ok(())
        })
        .unwrap();

    assert!(
        (&store)
            .update_task_status(parent.id.as_str(), crate::types::TaskStatus::Stopped)
            .unwrap()
    );

    assert!(launched);
    store.with_running_task(child.id.as_str(), || Ok(())).unwrap();
    assert_eq!(
        store.get_task(child.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Running
    );
    assert!(
        (&store)
            .update_task_status(child.id.as_str(), crate::types::TaskStatus::Stopped)
            .unwrap()
    );
}
