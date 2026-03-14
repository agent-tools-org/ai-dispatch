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
    pub multipane_mode: bool,
    pub active_pane: usize,
    pub pane_scroll_offsets: Vec<usize>,
    pub should_quit: bool,
    task_id_filter: Option<String>,
    group_filter: Option<String>,
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
            multipane_mode: false,
            active_pane: 0,
            pane_scroll_offsets: Vec::new(),
            should_quit: false,
            task_id_filter: options.task_id,
            group_filter: options.group,
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

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char('d') => {
                self.dashboard_mode = !self.dashboard_mode;
                return Ok(());
            }
            KeyCode::Char('m') => {
                self.multipane_mode = !self.multipane_mode;
                if self.multipane_mode {
                    self.active_pane = 0;
                    self.pane_scroll_offsets.clear();
                }
                return Ok(());
            }
            _ => {}
        }
        if self.multipane_mode {
            return self.handle_multipane_key(key);
        }
        if self.detail_mode {
            return self.handle_detail_key(key);
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Enter => self.enter_detail_mode()?,
            _ => {}
        }
        Ok(())
    }

    pub fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.selected)
    }

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
        match (self.task_id_filter.as_deref(), self.group_filter.as_deref()) {
            (Some(task_id), Some(group_id)) => format!("task {task_id} | group {group_id}"),
            (Some(task_id), None) => format!("task {task_id}"),
            (None, Some(group_id)) => format!("today | group {group_id}"),
            (None, None) => "today".to_string(),
        }
    }

    pub fn empty_message(&self) -> String {
        format!("No tasks matched scope: {}", self.scope_label())
    }

    fn handle_multipane_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Tab => {
                let pane_count = self.pane_count();
                if pane_count > 0 {
                    self.active_pane = (self.active_pane + 1) % pane_count;
                }
            }
            KeyCode::BackTab => {
                let pane_count = self.pane_count();
                if pane_count > 0 {
                    self.active_pane = if self.active_pane == 0 {
                        pane_count - 1
                    } else {
                        self.active_pane - 1
                    };
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.active_pane < self.pane_scroll_offsets.len() {
                    let offset = &mut self.pane_scroll_offsets[self.active_pane];
                    if *offset > 0 {
                        *offset -= 1;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.active_pane < self.pane_scroll_offsets.len() {
                    self.pane_scroll_offsets[self.active_pane] += 1;
                }
            }
            KeyCode::Enter => {
                let tasks = self.multipane_tasks();
                if let Some(task) = tasks.get(self.active_pane) {
                    if let Some(idx) = self.tasks.iter().position(|t| t.id == task.id) {
                        self.selected = idx;
                        self.multipane_mode = false;
                        self.enter_detail_mode()?;
                    }
                }
            }
            KeyCode::Esc => {
                self.multipane_mode = false;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('e') => self.set_detail_tab(DetailTab::Events),
            KeyCode::Char('p') => self.set_detail_tab(DetailTab::Prompt),
            KeyCode::Char('o') => self.set_detail_tab(DetailTab::Output),
            KeyCode::Tab => self.set_detail_tab(self.detail_tab.next()),
            KeyCode::BackTab => self.set_detail_tab(self.detail_tab.previous()),
            KeyCode::Down | KeyCode::Char('j') if self.detail_tab.is_text_view() => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.detail_tab.is_text_view() => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            KeyCode::Esc => {
                self.detail_mode = false;
                self.reset_detail_state();
            }
            _ => {}
        }
        Ok(())
    }

    fn enter_detail_mode(&mut self) -> Result<()> {
        self.detail_mode = true;
        self.reset_detail_state();
        self.load_selected_events()
    }

    fn reset_detail_state(&mut self) {
        self.detail_tab = DetailTab::Events;
        self.detail_scroll = 0;
    }

    fn set_detail_tab(&mut self, tab: DetailTab) {
        self.detail_tab = tab;
        self.detail_scroll = 0;
    }

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

        let mut tasks = self.store.list_tasks(TaskFilter::Today)?;
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

    fn next(&mut self) {
        if !self.tasks.is_empty() {
            self.selected = (self.selected + 1) % self.tasks.len();
        }
    }

    fn previous(&mut self) {
        if !self.tasks.is_empty() {
            self.selected = if self.selected == 0 {
                self.tasks.len() - 1
            } else {
                self.selected - 1
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::types::{AgentKind, TaskId, TaskStatus};

    fn make_task(id: &str, group_id: Option<&str>) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            prompt: format!("prompt for {id}"),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: group_id.map(str::to_string),
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn filters_today_view_by_group() {
        let store = Arc::new(Store::open_memory().unwrap());
        store
            .insert_task(&make_task("t-1000", Some("wg-a")))
            .unwrap();
        store
            .insert_task(&make_task("t-1001", Some("wg-b")))
            .unwrap();

        let app = App::new(
            store,
            super::super::RunOptions {
                task_id: None,
                group: Some("wg-a".to_string()),
            },
        )
        .unwrap();

        assert_eq!(app.tasks.len(), 1);
        assert_eq!(app.tasks[0].id.as_str(), "t-1000");
        assert_eq!(app.scope_label(), "today | group wg-a");
    }

    #[test]
    fn filters_specific_task_scope() {
        let store = Arc::new(Store::open_memory().unwrap());
        store
            .insert_task(&make_task("t-1000", Some("wg-a")))
            .unwrap();
        store
            .insert_task(&make_task("t-1001", Some("wg-b")))
            .unwrap();

        let app = App::new(
            store,
            super::super::RunOptions {
                task_id: Some("t-1001".to_string()),
                group: Some("wg-b".to_string()),
            },
        )
        .unwrap();

        assert_eq!(app.tasks.len(), 1);
        assert_eq!(app.tasks[0].id.as_str(), "t-1001");
        assert_eq!(app.scope_label(), "task t-1001 | group wg-b");
    }

    #[test]
    fn loads_running_task_milestone() {
        let store = Arc::new(Store::open_memory().unwrap());
        let mut task = make_task("t-1002", Some("wg-a"));
        task.status = TaskStatus::Running;
        store.insert_task(&task).unwrap();
        store
            .insert_event(&TaskEvent {
                task_id: task.id.clone(),
                timestamp: Local::now(),
                event_kind: crate::types::EventKind::Milestone,
                detail: "types defined".to_string(),
                metadata: None,
            })
            .unwrap();

        let app = App::new(
            store,
            super::super::RunOptions {
                task_id: None,
                group: Some("wg-a".to_string()),
            },
        )
        .unwrap();

        assert_eq!(app.get_milestone("t-1002"), Some("types defined"));
    }

    #[test]
    fn detail_mode_cycles_tabs_and_resets_scroll() {
        let store = Arc::new(Store::open_memory().unwrap());
        store.insert_task(&make_task("t-1003", None)).unwrap();
        let mut app = App::new(
            store,
            super::super::RunOptions {
                task_id: None,
                group: None,
            },
        )
        .unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(app.detail_mode);
        assert!(app.detail_tab == DetailTab::Events);

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
            .unwrap();
        assert!(app.detail_tab == DetailTab::Prompt);
        assert_eq!(app.detail_scroll, 1);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .unwrap();
        assert!(app.detail_tab == DetailTab::Output);
        assert_eq!(app.detail_scroll, 0);

        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
            .unwrap();
        assert!(app.detail_tab == DetailTab::Prompt);
    }

    #[test]
    fn detail_mode_keeps_selection_stable_and_resets_on_escape() {
        let store = Arc::new(Store::open_memory().unwrap());
        store.insert_task(&make_task("t-1004", None)).unwrap();
        store.insert_task(&make_task("t-1005", None)).unwrap();
        let mut app = App::new(
            store,
            super::super::RunOptions {
                task_id: None,
                group: None,
            },
        )
        .unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected, 0);
        assert_eq!(app.detail_scroll, 0);

        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.selected, 0);
        assert_eq!(app.detail_scroll, 0);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.detail_mode);
        assert!(app.detail_tab == DetailTab::Events);
        assert_eq!(app.detail_scroll, 0);
    }
}
