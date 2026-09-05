// App state for the aid task dashboard TUI.
// Loads task lists from Store and handles keyboard-driven navigation.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::metrics::ProcessMetrics;
use super::stats::{StatsRange, StatsSnapshot};
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskStatus};

#[path = "app_keys.rs"]
mod app_keys;
#[path = "app_tasks.rs"]
mod app_tasks;
#[path = "app_panes.rs"]
mod app_panes;
#[path = "app_navigation.rs"]
mod app_navigation;
#[path = "app_refresh.rs"]
mod app_refresh;
#[path = "stats_app.rs"]
mod stats_app;
#[path = "refresh/mod.rs"]
mod refresh;
pub(super) use refresh::RefreshWorker;

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
    pub nodes: Arc<Vec<super::tree_data::TreeNode>>,
    pub refresh_requested: bool,
    pub refresh_status: Option<String>,
    pub output_cache: HashMap<String, String>,
    pub events_cache: HashMap<String, Vec<TaskEvent>>,
    latest_events: HashMap<String, TaskEvent>,
    pub metrics: HashMap<String, ProcessMetrics>,
    pub milestones: HashMap<String, String>,
    pub selected: usize,
    pub detail_mode: bool,
    pub detail_tab: DetailTab,
    pub detail_scroll: usize,
    pub dashboard_mode: bool,
    pub stats_mode: bool,
    pub legacy_stats_view: bool,
    pub stats_range: StatsRange,
    pub stats: StatsSnapshot,
    pub multipane_mode: bool,
    pub tree_mode: bool,
    pub tree_selected: usize,
    pub tree_node_count: usize,
    pub collapsed_projects: std::collections::HashSet<Option<String>>,
    pub search_mode: bool,
    pub search_query: String,
    pub wg_creators: HashMap<String, String>,
    pub show_all: bool,
    pub active_pane: usize,
    pub pane_scroll_offsets: HashMap<String, usize>,
    pub should_quit: bool,
    task_id_filter: Option<String>,
    group_filter: Option<String>,
    config: crate::config::AidConfig,
    store: Arc<Store>,
    cached_terminal_milestones: HashMap<String, String>,
    active_pane_task_id: Option<String>,
    last_task_refresh: Instant,
    pub(crate) animation_phase: u8,
}

impl App {
    #[cfg(test)]
    pub fn new(store: Arc<Store>, options: super::RunOptions) -> Result<Self> {
        let mut app = Self::empty(store, options);
        app.reload_tasks()?;
        Ok(app)
    }

    pub(super) fn empty(store: Arc<Store>, options: super::RunOptions) -> Self {
        Self {
            tasks: Vec::new(),
            nodes: Arc::new(Vec::new()),
            refresh_requested: true,
            refresh_status: Some("Loading tasks…".into()),
            output_cache: HashMap::new(),
            events_cache: HashMap::new(),
            latest_events: HashMap::new(),
            metrics: HashMap::new(),
            milestones: HashMap::new(),
            selected: 0,
            detail_mode: false,
            detail_tab: DetailTab::Events,
            detail_scroll: 0,
            dashboard_mode: false,
            stats_mode: false,
            legacy_stats_view: false,
            stats_range: StatsRange::AllTime,
            stats: StatsSnapshot::empty(StatsRange::AllTime),
            multipane_mode: false,
            tree_mode: false,
            tree_selected: 0,
            tree_node_count: 0,
            collapsed_projects: std::collections::HashSet::new(),
            search_mode: false,
            search_query: String::new(),
            wg_creators: HashMap::new(),
            show_all: false,
            active_pane: 0,
            pane_scroll_offsets: HashMap::new(),
            should_quit: false,
            task_id_filter: options.task_id,
            group_filter: options.group,
            config: crate::config::load_config().unwrap_or_default(),
            store,
            cached_terminal_milestones: HashMap::new(),
            active_pane_task_id: None,
            last_task_refresh: Instant::now(),
            animation_phase: 0,
        }
    }

    pub fn tick(&mut self) -> Result<()> {
        if self.last_task_refresh.elapsed() >= TASK_REFRESH_INTERVAL {
            self.refresh_requested = true;
            self.last_task_refresh = Instant::now();
        }
        if self.has_reasoning_task() {
            self.animation_phase = self.animation_phase.wrapping_add(1) % 3;
        }
        Ok(())
    }

    pub fn selected_task(&self) -> Option<&Task> { self.tasks.get(self.selected) }
    pub fn selected_events(&self) -> &[TaskEvent] {
        self.selected_task()
            .and_then(|task| self.events_cache.get(task.id.as_str()))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
    pub fn get_metrics(&self, task_id: &str) -> Option<&ProcessMetrics> {
        self.metrics.get(task_id)
    }
    pub fn get_milestone(&self, task_id: &str) -> Option<&str> {
        self.milestones.get(task_id).map(String::as_str)
    }
    pub fn task_activity(&self, task: &Task) -> String {
        let label = super::agent_state::activity_label(
            task.status,
            task.agent,
            task.id.as_str(),
            self.latest_events.get(task.id.as_str()),
        );
        if label.starts_with("THINKING ·") {
            format!("{label} {}", ["·", "··", "···"][self.animation_phase as usize])
        } else {
            label
        }
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
        tasks.truncate(6);
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
        let scope = if self.show_all && self.task_id_filter.is_none() { "all" } else { "today+active" };
        let project = "project:*";
        match (self.task_id_filter.as_deref(), self.group_filter.as_deref()) {
            (Some(t), Some(g)) => format!("{project} | task {t} | group {g}"),
            (Some(t), None) => format!("{project} | task {t}"),
            (None, Some(g)) => format!("{project} | {scope} | group {g}"),
            (None, None) => format!("{project} | {scope}"),
        }
    }
    pub fn empty_message(&self) -> String {
        self.refresh_status.clone().unwrap_or_else(|| format!("No tasks matched scope: {}", self.scope_label()))
    }
}

const TASK_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "app_selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "app_group_tests.rs"]
mod group_tests;

#[cfg(test)]
#[path = "app_detail_tests.rs"]
mod detail_tests;
