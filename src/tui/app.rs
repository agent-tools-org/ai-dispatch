// App state for the aid task dashboard TUI.
// Loads task lists from Store and handles keyboard-driven navigation.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;
use std::sync::Arc;

use super::metrics::{get_process_metrics, ProcessMetrics};
use crate::background;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskFilter, TaskStatus};

#[path = "app_keys.rs"]
mod app_keys;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Events,
    Prompt,
    Output,
}

impl DetailTab {
    fn next(self) -> Self {
        match self {
            Self::Events => Self::Prompt,
            Self::Prompt => Self::Output,
            Self::Output => Self::Events,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Events => Self::Output,
            Self::Prompt => Self::Events,
            Self::Output => Self::Prompt,
        }
    }

    fn is_text_view(self) -> bool {
        matches!(self, Self::Prompt | Self::Output)
    }
}

pub struct App {
    pub tasks: Vec<Task>,
    pub events_cache: HashMap<String, Vec<TaskEvent>>,
    pub metrics: HashMap<String, ProcessMetrics>,
    pub milestones: HashMap<String, String>,
    pub selected: usize,
    pub detail_mode: bool,
    pub detail_tab: DetailTab,
    pub detail_scroll: usize,
    pub dashboard_mode: bool,
    pub stats_mode: bool,
    pub multipane_mode: bool,
    pub show_all: bool,
    pub active_pane: usize,
    pub pane_scroll_offsets: Vec<usize>,
    pub should_quit: bool,
    task_id_filter: Option<String>,
    group_filter: Option<String>,
    config: crate::config::AidConfig,
    store: Arc<Store>,
}

impl App {
    pub fn new(store: Arc<Store>, options: super::RunOptions) -> Result<Self> {
        let mut app = Self {
            tasks: Vec::new(),
            events_cache: HashMap::new(),
            metrics: HashMap::new(),
            milestones: HashMap::new(),
            selected: 0,
            detail_mode: false,
            detail_tab: DetailTab::Events,
            detail_scroll: 0,
            dashboard_mode: false,
            stats_mode: false,
            multipane_mode: false,
            show_all: false,
            active_pane: 0,
            pane_scroll_offsets: Vec::new(),
            should_quit: false,
            task_id_filter: options.task_id,
            group_filter: options.group,
            config: crate::config::load_config().unwrap_or_default(),
            store,
        };
        app.reload_tasks()?;
        Ok(app)
    }

    pub fn tick(&mut self) -> Result<()> {
        self.reload_tasks()?;
        if self.dashboard_mode {
            self.load_dashboard_events()?;
        }
        if self.multipane_mode {
            self.load_multipane_events()?;
            let count = self.multipane_tasks().len();
            self.pane_scroll_offsets.resize(count, 0);
        }
        if self.detail_mode {
            self.load_selected_events()?;
        }
        Ok(())
    }

    pub fn selected_task(&self) -> Option<&Task> { self.tasks.get(self.selected) }
    pub fn selected_events(&self) -> Vec<TaskEvent> {
        self.selected_task()
            .and_then(|task| self.events_cache.get(task.id.as_str()))
            .cloned()
            .unwrap_or_default()
    }
    pub fn get_metrics(&self, task_id: &str) -> Option<&ProcessMetrics> {
        self.metrics.get(task_id)
    }
    pub fn get_milestone(&self, task_id: &str) -> Option<&str> {
        self.milestones.get(task_id).map(String::as_str)
    }
    pub fn config(&self) -> &crate::config::AidConfig { &self.config }
    pub fn task_milestones(&self, task_id: &str) -> Vec<String> {
        self.events_cache
            .get(task_id)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.event_kind == EventKind::Milestone)
                    .map(|event| event.detail.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn multipane_tasks(&self) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    TaskStatus::Running
                        | TaskStatus::AwaitingInput
                        | TaskStatus::Done
                        | TaskStatus::Merged
                        | TaskStatus::Failed
                )
            })
            .collect();
        tasks.sort_by(|a, b| {
            let running_a = matches!(a.status, TaskStatus::Running | TaskStatus::AwaitingInput);
            let running_b = matches!(b.status, TaskStatus::Running | TaskStatus::AwaitingInput);
            running_b
                .cmp(&running_a)
                .then(b.created_at.cmp(&a.created_at))
        });
        tasks
    }
    pub fn pane_count(&self) -> usize {
        self.multipane_tasks().len().min(6)
    }
    pub fn scope_label(&self) -> String {
        let scope = if self.show_all && self.task_id_filter.is_none() {
            "all"
        } else {
            "today"
        };
        match (self.task_id_filter.as_deref(), self.group_filter.as_deref()) {
            (Some(task_id), Some(group_id)) => format!("task {task_id} | group {group_id}"),
            (Some(task_id), None) => format!("task {task_id}"),
            (None, Some(group_id)) => format!("{scope} | group {group_id}"),
            (None, None) => scope.to_string(),
        }
    }
    pub fn empty_message(&self) -> String { format!("No tasks matched scope: {}", self.scope_label()) }
    fn load_selected_events(&mut self) -> Result<()> {
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
    fn load_dashboard_events(&mut self) -> Result<()> {
        for task_id in self
            .tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Running | TaskStatus::AwaitingInput
                )
            })
            .map(|task| task.id.as_str().to_string())
        {
            self.events_cache
                .insert(task_id.clone(), self.store.get_events(&task_id)?);
        }
        Ok(())
    }
    fn load_multipane_events(&mut self) -> Result<()> {
        let task_ids: Vec<String> = self
            .multipane_tasks()
            .iter()
            .map(|t| t.id.as_str().to_string())
            .collect();
        for task_id in task_ids {
            // Always refresh running tasks, cache completed ones
            let is_running = self.tasks.iter().any(|t| {
                t.id.as_str() == task_id
                    && matches!(t.status, TaskStatus::Running | TaskStatus::AwaitingInput)
            });
            if is_running || !self.events_cache.contains_key(&task_id) {
                self.events_cache
                    .insert(task_id.clone(), self.store.get_events(&task_id)?);
            }
        }
        Ok(())
    }
    fn reload_tasks(&mut self) -> Result<()> {
        let tasks = self.load_tasks()?;
        self.metrics = self.load_metrics(&tasks);
        self.milestones = self.load_milestones(&tasks)?;
        self.tasks = tasks;
        if self.selected >= self.tasks.len() && !self.tasks.is_empty() {
            self.selected = self.tasks.len() - 1;
        }
        Ok(())
    }
    fn load_tasks(&self) -> Result<Vec<Task>> {
        if let Some(task_id) = self.task_id_filter.as_deref() {
            return self.load_task_scope(task_id);
        }
        let filter = if self.show_all {
            TaskFilter::All
        } else {
            TaskFilter::Today
        };
        let mut tasks = self.store.list_tasks(filter)?;
        self.apply_group_filter(&mut tasks);
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
            tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
        }
    }
    fn load_metrics(&self, tasks: &[Task]) -> HashMap<String, ProcessMetrics> {
        let mut metrics = HashMap::new();
        for task in tasks.iter().filter(|task| {
            matches!(
                task.status,
                crate::types::TaskStatus::Running | crate::types::TaskStatus::AwaitingInput
            )
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
    fn load_milestones(&self, tasks: &[Task]) -> Result<HashMap<String, String>> {
        let mut milestones = HashMap::new();
        for task in tasks.iter().filter(|task| {
            matches!(
                task.status,
                crate::types::TaskStatus::Running | crate::types::TaskStatus::AwaitingInput
            )
        }) {
            if let Some(milestone) = self.store.latest_milestone(task.id.as_str())? {
                milestones.insert(task.id.as_str().to_string(), milestone);
            }
        }
        Ok(milestones)
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
