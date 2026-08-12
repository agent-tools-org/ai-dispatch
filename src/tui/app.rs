// App state for the aid task dashboard TUI.
// Loads task lists from Store and handles keyboard-driven navigation.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::metrics::ProcessMetrics;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskStatus};

#[path = "app_keys.rs"]
mod app_keys;
#[path = "app_tasks.rs"]
mod app_tasks;
#[path = "app_panes.rs"]
mod app_panes;

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
    pub tree_mode: bool,
    pub tree_selected: usize,
    pub tree_node_count: usize,
    pub wg_creators: HashMap<String, String>,
    pub show_all: bool,
    pub active_pane: usize,
    pub pane_scroll_offsets: HashMap<String, usize>,
    pub should_quit: bool,
    task_id_filter: Option<String>,
    group_filter: Option<String>,
    config: crate::config::AidConfig,
    store: Arc<Store>,
    last_metrics_refresh: Instant,
    cached_terminal_milestones: HashMap<String, String>,
    active_pane_task_id: Option<String>,
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
            tree_mode: false,
            tree_selected: 0,
            tree_node_count: 0,
            wg_creators: HashMap::new(),
            show_all: false,
            active_pane: 0,
            pane_scroll_offsets: HashMap::new(),
            should_quit: false,
            task_id_filter: options.task_id,
            group_filter: options.group,
            config: crate::config::load_config().unwrap_or_default(),
            store,
            last_metrics_refresh: Instant::now(),
            cached_terminal_milestones: HashMap::new(),
            active_pane_task_id: None,
        };
        app.reload_tasks()?;
        Ok(app)
    }

    pub fn tick(&mut self) -> Result<()> {
        self.reload_tasks()?;
        // Only refresh process metrics every 2 seconds (ps fork is expensive)
        if self.last_metrics_refresh.elapsed().as_secs() >= 2 {
            self.metrics = self.load_metrics(&self.tasks);
            self.last_metrics_refresh = Instant::now();
        }
        if self.dashboard_mode {
            self.load_dashboard_events()?;
        }
        if self.multipane_mode {
            self.reconcile_active_pane();
            self.load_multipane_events()?;
            self.clamp_all_pane_scrolls();
        }
        if self.detail_mode {
            self.load_selected_events()?;
            self.clamp_detail_scroll();
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
    pub fn get_failure_reason(&self, task_id: &str) -> Option<String> {
        self.events_cache.get(task_id).and_then(|events| {
            events
                .iter()
                .rev()
                .find(|e| e.event_kind == EventKind::Error)
                .map(|e| e.detail.clone())
        })
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
        let mut tasks: Vec<&Task> = self.tasks.iter().collect();
        tasks.sort_by(|a, b| {
            let running_a = matches!(
                a.status,
                TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
            );
            let running_b = matches!(
                b.status,
                TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
            );
            running_b
                .cmp(&running_a)
                .then(b.created_at.cmp(&a.created_at))
        });
        tasks
    }
    pub fn pane_count(&self) -> usize {
        self.multipane_tasks().len().min(6)
    }
    pub(crate) fn pane_scroll_offset(&self, task_id: &str) -> usize {
        self.pane_scroll_offsets.get(task_id).copied().unwrap_or(0)
    }
    fn reconcile_active_pane(&mut self) {
        let tasks = self.multipane_tasks();
        if tasks.is_empty() {
            self.active_pane = 0;
            self.active_pane_task_id = None;
            return;
        }
        let index = self
            .active_pane_task_id
            .as_deref()
            .and_then(|id| tasks.iter().position(|task| task.id.as_str() == id))
            .unwrap_or_else(|| self.active_pane.min(tasks.len() - 1));
        let task_id = tasks[index].id.as_str().to_string();
        self.active_pane = index;
        self.active_pane_task_id = Some(task_id);
    }
    pub fn scope_label(&self) -> String {
        let scope = if self.show_all && self.task_id_filter.is_none() {
            "all"
        } else {
            "today+active"
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
                    && matches!(
                        t.status,
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
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "app_selection_tests.rs"]
mod selection_tests;
