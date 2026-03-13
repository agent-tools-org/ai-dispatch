// App state for the aid task dashboard TUI.
// Loads task lists from Store and handles keyboard-driven navigation.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;
use std::sync::Arc;

use super::metrics::{ProcessMetrics, get_process_metrics};
use crate::background;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskFilter};

pub struct App {
    pub tasks: Vec<Task>,
    pub events_cache: HashMap<String, Vec<TaskEvent>>,
    pub metrics: HashMap<String, ProcessMetrics>,
    pub milestones: HashMap<String, String>,
    pub selected: usize,
    pub detail_mode: bool,
    pub dashboard_mode: bool,
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
            dashboard_mode: false,
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
        if self.detail_mode {
            self.load_selected_events()?;
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('d') => self.dashboard_mode = !self.dashboard_mode,
            KeyCode::Down | KeyCode::Char('j') if !self.detail_mode => self.next(),
            KeyCode::Up | KeyCode::Char('k') if !self.detail_mode => self.previous(),
            KeyCode::Enter if !self.detail_mode => {
                self.detail_mode = true;
                self.load_selected_events()?;
            }
            KeyCode::Esc if self.detail_mode => self.detail_mode = false,
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
        self.events_cache.get(task_id).map(|events| events.iter().filter(|event| event.event_kind == EventKind::Milestone).map(|event| event.detail.clone()).collect()).unwrap_or_default()
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

    fn load_selected_events(&mut self) -> Result<()> {
        let Some(task_id) = self.selected_task().map(|task| task.id.as_str().to_string()) else {
            return Ok(());
        };
        let events = self.store.get_events(&task_id)?;
        self.events_cache.insert(task_id, events);
        Ok(())
    }

    fn load_dashboard_events(&mut self) -> Result<()> {
        for task_id in self.tasks.iter().filter(|task| matches!(task.status, crate::types::TaskStatus::Running | crate::types::TaskStatus::AwaitingInput)).map(|task| task.id.as_str().to_string()) {
            self.events_cache.insert(task_id.clone(), self.store.get_events(&task_id)?);
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
        let mut tasks = self.store.get_task(task_id)?.into_iter().collect::<Vec<_>>();
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
        for task in tasks
            .iter()
            .filter(|task| matches!(task.status, crate::types::TaskStatus::Running | crate::types::TaskStatus::AwaitingInput))
        {
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
        for task in tasks
            .iter()
            .filter(|task| matches!(task.status, crate::types::TaskStatus::Running | crate::types::TaskStatus::AwaitingInput))
        {
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
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
        }
    }

    #[test]
    fn filters_today_view_by_group() {
        let store = Arc::new(Store::open_memory().unwrap());
        store.insert_task(&make_task("t-1000", Some("wg-a"))).unwrap();
        store.insert_task(&make_task("t-1001", Some("wg-b"))).unwrap();

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
        store.insert_task(&make_task("t-1000", Some("wg-a"))).unwrap();
        store.insert_task(&make_task("t-1001", Some("wg-b"))).unwrap();

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
        store.insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: crate::types::EventKind::Milestone,
            detail: "types defined".to_string(),
            metadata: None,
        }).unwrap();

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
}
