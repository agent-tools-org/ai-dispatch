// Additional regression coverage for tests.rs.
// Deps: parent test module fixtures.

use super::*;

#[test]
fn best_of_task_ids_fall_back_to_random_when_derived_id_is_running() {
    let store = Store::open_memory().unwrap();
    let base = TaskId("t-batch".into());
    store
        .insert_waiting_task(
            base.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status(base.as_str(), TaskStatus::Pending)
        .unwrap();
    store
        .insert_waiting_task(
            "t-batch-bo2",
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status("t-batch-bo2", TaskStatus::Running)
        .unwrap();
    let candidate = best_of_task_id(&store, Some(&base), 1).unwrap();
    assert_eq!(candidate, None);
}

#[test]
fn best_of_task_ids_drop_invalid_auto_suffixes() {
    let store = Store::open_memory().unwrap();
    let base = TaskId(format!("t-{}", "a".repeat(62)));
    let derived = format!("t-{}-bo5", "a".repeat(58));
    store
        .insert_waiting_task(
            base.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status(base.as_str(), TaskStatus::Pending)
        .unwrap();
    store
        .insert_waiting_task(
            &derived, "codex", "prompt", None, None, None, None, None, None, false, false,
        )
        .unwrap();
    store
        .update_task_status(&derived, TaskStatus::Pending)
        .unwrap();
    let candidate = best_of_task_id(&store, Some(&base), 4).unwrap();
    assert_eq!(candidate, None);
}

#[test]
fn best_of_task_ids_reject_invalid_base_ids_before_reuse() {
    let store = Store::open_memory().unwrap();
    let err = best_of_task_id(&store, Some(&TaskId("-bad".into())), 0).unwrap_err();
    assert!(err.to_string().contains("Invalid task ID"));
}
