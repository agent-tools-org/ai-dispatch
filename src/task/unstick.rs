// Shared helpers for manual and automatic unstick flows.
// Exports: default_nudge_message, mark_task_stalled, and queue_auto_nudge.
// Deps: crate::runtime::pty::watch_idle, crate::store::Store, crate::types.

use anyhow::Result;

use crate::runtime::pty::watch_idle;
use crate::store::Store;
use crate::types::{MessageDirection, MessageSource, TaskStatus};

pub fn default_nudge_message() -> String {
    watch_idle::default_nudge_message()
}

pub fn queue_auto_nudge(store: &Store, task_id: &str, message: &str) -> Result<()> {
    store.insert_message(
        task_id,
        MessageDirection::In,
        message,
        MessageSource::UnstickAuto,
    )?;
    Ok(())
}

pub fn mark_task_stalled(store: &Store, task_id: &str) -> Result<bool> {
    let Some(task) = store.get_task(task_id)? else {
        return Ok(false);
    };
    if task.status != TaskStatus::Running {
        return Ok(false);
    }
    crate::task::lifecycle::mark_stalled(store, task_id)
}

#[cfg(test)]
mod tests {


    use super::*;
    use crate::store::Store;
    use crate::types::{Task, TaskId};

    fn make_task(id: &str, status: TaskStatus) -> Task {
        crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }

    #[test]
    fn helper_marks_running_task_stalled() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-stalled", TaskStatus::Running)).unwrap();

        assert!(mark_task_stalled(&store, "t-stalled").unwrap());
        assert_eq!(
            store.get_task("t-stalled").unwrap().unwrap().status,
            TaskStatus::Stalled
        );
    }

    #[test]
    fn helper_queues_auto_nudge_message() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-nudge", TaskStatus::Running)).unwrap();

        queue_auto_nudge(&store, "t-nudge", "hello").unwrap();

        let messages = store.list_messages_for_task("t-nudge").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].source, MessageSource::UnstickAuto);
    }
}
