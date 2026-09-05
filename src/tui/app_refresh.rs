// Scheduled event refreshes for the TUI app.
// Exports App helpers that keep database work out of render calls.
// Deps: App task state, Store event queries, and agent EventKind values.

use super::*;

impl App {
    pub(super) fn load_selected_events(&mut self) -> Result<()> {
        let Some(task_id) = self
            .selected_task()
            .map(|task| task.id.as_str().to_string())
        else {
            return Ok(());
        };
        let events = self.store.get_events(&task_id)?;
        self.events_cache.insert(task_id, events);
        Ok(())
    }

    pub(super) fn load_dashboard_events(&mut self) -> Result<()> {
        for task_id in self
            .tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
                )
            })
            .map(|task| task.id.as_str().to_string())
        {
            self.events_cache
                .insert(task_id.clone(), self.store.get_events(&task_id)?);
        }
        Ok(())
    }

    pub(super) fn load_multipane_events(&mut self) -> Result<()> {
        let task_ids: Vec<String> = self
            .multipane_tasks()
            .iter()
            .take(6)
            .map(|task| task.id.as_str().to_string())
            .collect();
        for task_id in task_ids {
            let is_running = self.tasks.iter().any(|task| {
                task.id.as_str() == task_id
                    && matches!(
                        task.status,
                        TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
                    )
            });
            if is_running || !self.events_cache.contains_key(&task_id) {
                self.events_cache
                    .insert(task_id.clone(), self.store.get_events(&task_id)?);
            }
        }
        Ok(())
    }

    pub(super) fn has_reasoning_task(&self) -> bool {
        self.tasks.iter().any(|task| {
            task.status == TaskStatus::Running
                && self
                    .latest_events
                    .get(task.id.as_str())
                    .is_some_and(|event| event.event_kind == EventKind::Reasoning)
        })
    }
}
