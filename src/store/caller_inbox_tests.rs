// Caller inbox: idempotent records, a single push claim, and caller-only acknowledgement.
// Uses in-memory stores with fixture tasks so foreign keys hold.
use super::*;
use crate::types::{Task, TaskId, TaskStatus};

fn store_with(ids: &[&str]) -> Store {
    let store = Store::open_memory().expect("store");
    for id in ids {
        store
            .insert_task(&Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done))
            .expect("task");
    }
    store
}

#[test]
fn push_is_claimed_once_and_released_on_failure() {
    let store = store_with(&["t-a"]);
    store.inbox_record("t-a", "s1").expect("record");
    store.inbox_record("t-a", "s1").expect("idempotent");
    assert!(store.inbox_claim_push("t-a").expect("claim"));
    assert!(!store.inbox_claim_push("t-a").expect("second claim"));
    store.inbox_release_push("t-a").expect("release");
    assert_eq!(store.inbox_pushed_at("t-a").expect("read"), None);
    assert!(store.inbox_claim_push("t-a").expect("claim after release"));
}

#[test]
fn only_the_caller_session_acknowledges_and_pending_lists_unread() {
    let store = store_with(&["t-a", "t-b", "t-c"]);
    store.inbox_record("t-a", "s1").expect("record");
    store.inbox_record("t-b", "s1").expect("record");
    store.inbox_record("t-c", "s2").expect("record");
    store.inbox_mark_seen("t-a", "s2").expect("foreign ack");
    let ids = |session| {
        store
            .inbox_pending(session, 10)
            .expect("pending")
            .into_iter()
            .map(|item| item.task_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids("s1"), ["t-a", "t-b"]);
    store.inbox_mark_seen("t-a", "s1").expect("ack");
    assert_eq!(ids("s1"), ["t-b"]);
    assert_eq!(ids("s2"), ["t-c"]);
}

#[test]
fn deleting_a_task_drops_its_inbox_row() {
    let store = store_with(&["t-a"]);
    store.inbox_record("t-a", "s1").expect("record");
    store.db().execute("DELETE FROM tasks WHERE id = 't-a'", []).expect("delete");
    assert!(store.inbox_pending("s1", 10).expect("pending").is_empty());
}
