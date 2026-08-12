// Multipane identity state and scroll helpers for the TUI App.
// Exports App methods for focus reconciliation and task-keyed pane scrolling.
// Deps: App task sorting and cached task events.

use super::*;

impl App {
    pub(super) fn clamp_all_pane_scrolls(&mut self) {
        let task_ids: Vec<String> = self
            .multipane_tasks()
            .iter()
            .map(|task| task.id.as_str().to_string())
            .collect();
        for task_id in task_ids {
            let max = self.pane_max_scroll_for_task(&task_id);
            if let Some(offset) = self.pane_scroll_offsets.get_mut(&task_id) {
                *offset = (*offset).min(max);
            }
        }
    }

    pub(super) fn scroll_pane_by(&mut self, pane: usize, delta: usize) {
        let Some(task_id) = self
            .multipane_tasks()
            .get(pane)
            .map(|task| task.id.as_str().to_string())
        else {
            return;
        };
        let max = self.pane_max_scroll_for_task(&task_id);
        let offset = self.pane_scroll_offsets.entry(task_id).or_default();
        *offset = (*offset).saturating_add(delta).min(max);
    }

    pub(super) fn pane_max_scroll(&self, pane: usize) -> usize {
        let task_id = self
            .multipane_tasks()
            .get(pane)
            .map(|task| task.id.as_str().to_string());
        let Some(task_id) = task_id else {
            return 0;
        };
        self.pane_max_scroll_for_task(&task_id)
    }

    fn pane_max_scroll_for_task(&self, task_id: &str) -> usize {
        let n = self
            .events_cache
            .get(task_id)
            .map(|events| events.len())
            .unwrap_or(0);
        n.saturating_sub(1)
    }

    pub(super) fn current_pane_task_id(&self) -> Option<String> {
        self.active_pane_task_id.clone()
    }

    pub(super) fn set_active_pane(&mut self, index: usize) {
        let task_id = self
            .multipane_tasks()
            .get(index)
            .map(|task| task.id.as_str().to_string());
        let Some(task_id) = task_id else {
            return;
        };
        self.active_pane = index;
        self.active_pane_task_id = Some(task_id);
    }
}
