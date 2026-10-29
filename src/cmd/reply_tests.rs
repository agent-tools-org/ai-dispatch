// Tests for tracked reply queuing and PTY write polling.
// Deps: reply handlers, Store, temporary AID homes, and task types.

use std::time::Duration;



use super::{InputCommand, ReplyOutcome, run, run_with_hook, run_with_hook_and_sleep};

use crate::store::Store;
use crate::types::{AgentKind, MessageSource, Task, TaskId, TaskStatus};

fn make_task(id: &str, status: TaskStatus) -> Task {
    Task {
        agent: AgentKind::Cursor,
        prompt: "test".to_string(),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

#[test]
fn reply_async_returns_immediately() {
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    store.insert_task(&make_task("t-reply-async", TaskStatus::Running)).unwrap();

    let outcome = run(&store, "t-reply-async", Some("follow this path"), None, true, 30).unwrap();

    assert_eq!(outcome, ReplyOutcome::Queued { id: 1 });
    let messages = store.list_messages_for_task("t-reply-async").unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source, MessageSource::Reply);
    assert_eq!(messages[0].content, "follow this path");
}

#[test]
fn reply_polls_until_written() {
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    store.insert_task(&make_task("t-reply-ack", TaskStatus::AwaitingInput)).unwrap();

    let mut polls = 0usize;
    let outcome = run_with_hook_and_sleep(
        &store,
        "t-reply-ack",
        Some("answer"),
        None,
        false,
        Duration::from_millis(25),
        Duration::from_millis(1),
        MessageSource::Reply,
        InputCommand::Reply,
        |message_id| {
            polls += 1;
            if polls == 1 {
                store.mark_delivered(message_id).unwrap();
            }
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(outcome, ReplyOutcome::Written);
}

#[test]
fn reply_times_out_cleanly() {
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    store.insert_task(&make_task("t-reply-timeout", TaskStatus::Stalled)).unwrap();

    let outcome = run_with_hook(
        &store,
        "t-reply-timeout",
        Some("nudge"),
        None,
        false,
        Duration::from_millis(5),
        Duration::from_millis(1),
        MessageSource::Reply,
        InputCommand::Reply,
        |_| {},
    )
    .unwrap();

    assert_eq!(outcome, ReplyOutcome::TimedOut);
}
