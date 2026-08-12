// Task loading helpers for the TUI App state.
// Exports App methods for scoped task lists, metrics, and milestone caches.
// Deps: Store queries, background worker pid lookup, and process metrics.

use anyhow::Result;
use std::collections::{HashMap, HashSet};

use super::App;
use super::super::metrics::{get_process_metrics, ProcessMetrics};
use crate::background;
use crate::types::{Task, TaskFilter, TaskStatus};

impl App {
    pub(super) fn reload_tasks(&mut self) -> Result<()> {
        let tree_nodes =
            super::super::tree_data::build_task_tree_with_state(
                &self.tasks,
                &self.wg_creators,
                &self.collapsed_projects,
            );
        let tree_anchor = tree_nodes
            .get(self.tree_selected)
            .map(|n| {
                (
                    n.task.id.as_str().to_string(),
                    n.project_id.clone(),
                    n.is_group_header,
                )
            });
        // Selection is identity-based: follow the selected task across reorders.
        let selected_id = self
            .tasks
            .get(self.selected)
            .map(|task| task.id.as_str().to_string());
        let prev_selected = self.selected;
        let tasks = self.load_tasks()?;
        self.milestones = self.load_milestones_batch(&tasks)?;
        self.latest_events = self.load_latest_events_batch(&tasks)?;
        if let Ok(wgs) = self.store.list_workgroups() {
            self.wg_creators = wgs
                .into_iter()
                .filter_map(|w| w.created_by.map(|by| (w.id.to_string(), by)))
                .collect();
        }
        self.tasks = tasks;
        self.selected = resolve_selected_index(&self.tasks, selected_id.as_deref(), prev_selected);
        let tree_nodes =
            super::super::tree_data::build_task_tree_with_state(
                &self.tasks,
                &self.wg_creators,
                &self.collapsed_projects,
            );
        self.tree_node_count = tree_nodes.len();
        self.tree_selected = App::resolve_tree_selected(
            &tree_nodes,
            tree_anchor,
            self.tree_selected,
        );
        if self.multipane_mode {
            self.reconcile_active_pane();
        }
        Ok(())
    }

    /// When the previously selected task is gone, keep a nearby clamped index
    /// (not jump to 0). Empty list → index 0.
    pub(super) fn resolve_tree_selected(
        nodes: &[super::super::tree_data::TreeNode],
        anchor: Option<(String, Option<String>, bool)>,
        prev: usize,
    ) -> usize {
        if nodes.is_empty() {
            return 0;
        }
        if let Some((id, project_id, is_header)) = anchor.as_ref() {
            if let Some(idx) = nodes
                .iter()
                .position(|n| {
                    n.task.id.as_str() == id
                        && n.project_id.as_ref() == project_id.as_ref()
                        && n.is_group_header == *is_header
                })
            {
                return idx;
            }
            if *is_header
                && let Some(idx) = nodes.iter().position(|node| {
                    node.is_group_header && node.project_id.as_ref() == project_id.as_ref()
                })
            {
                return idx;
            }
            if let Some(idx) = nodes.iter().position(|n| n.task.id.as_str() == id) {
                return idx;
            }
        }
        prev.min(nodes.len() - 1)
    }

    fn load_tasks(&self) -> Result<Vec<Task>> {
        if let Some(task_id) = self.task_id_filter.as_deref() {
            return self.load_task_scope(task_id);
        }
        let mut tasks = if self.show_all {
            self.store.list_tasks(TaskFilter::All)?
        } else {
            self.load_today_with_active_tasks()?
        };
        self.apply_group_filter(&mut tasks);
        Ok(tasks)
    }

    fn load_today_with_active_tasks(&self) -> Result<Vec<Task>> {
        let mut tasks = self.store.list_tasks(TaskFilter::Today)?;
        let mut seen: HashSet<String> = tasks.iter().map(|task| task.id.0.clone()).collect();
        for task in self.store.list_tasks(TaskFilter::Active)? {
            if seen.insert(task.id.0.clone()) {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }

    fn load_task_scope(&self, task_id: &str) -> Result<Vec<Task>> {
        let mut tasks = self
            .store
            .get_task(task_id)?
            .into_iter()
            .collect::<Vec<_>>();
        self.apply_group_filter(&mut tasks);
        Ok(tasks)
    }

    fn apply_group_filter(&self, tasks: &mut Vec<Task>) {
        if let Some(group_id) = self.group_filter.as_deref() {
            tasks.retain(|task| {
                task.workgroup_id.as_deref() == Some(group_id) || task.workgroup_id.is_none()
            });
        }
    }

    pub(super) fn load_metrics(&self, tasks: &[Task]) -> HashMap<String, ProcessMetrics> {
        let mut metrics = HashMap::new();
        for task in tasks.iter().filter(|task| {
            matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput)
        }) {
            let Ok(Some(pid)) = background::load_worker_pid(task.id.as_str()) else {
                continue;
            };
            let Some(process_metrics) = get_process_metrics(pid) else {
                continue;
            };
            metrics.insert(task.id.as_str().to_string(), process_metrics);
        }
        metrics
    }

    fn load_milestones_batch(&mut self, tasks: &[Task]) -> Result<HashMap<String, String>> {
        let mut need_query: Vec<&str> = Vec::new();
        let mut result = HashMap::new();
        for task in tasks.iter().filter(|task| task.status != TaskStatus::Pending) {
            if task.status.is_terminal()
                && let Some(cached) = self.cached_terminal_milestones.get(task.id.as_str())
            {
                result.insert(task.id.as_str().to_string(), cached.clone());
                continue;
            }
            need_query.push(task.id.as_str());
        }
        if need_query.is_empty() {
            return Ok(result);
        }
        let fresh = self.store.latest_milestones_batch(&need_query)?;
        for (task_id, detail) in &fresh {
            if let Some(task) = tasks.iter().find(|task| task.id.as_str() == task_id)
                && task.status.is_terminal()
            {
                self.cached_terminal_milestones
                    .insert(task_id.clone(), detail.clone());
            }
        }
        result.extend(fresh);
        Ok(result)
    }

    fn load_latest_events_batch(&self, tasks: &[Task]) -> Result<HashMap<String, crate::types::TaskEvent>> {
        let task_ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        self.store.latest_events_batch(&task_ids)
    }
}

fn resolve_selected_index(tasks: &[Task], selected_id: Option<&str>, prev: usize) -> usize {
    if tasks.is_empty() {
        return 0;
    }
    if let Some(id) = selected_id {
        if let Some(idx) = tasks.iter().position(|task| task.id.as_str() == id) {
            return idx;
        }
        // Task genuinely gone: clamp previous index into the new list.
        return prev.min(tasks.len() - 1);
    }
    0
}
