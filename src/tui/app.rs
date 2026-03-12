// App state for the aid task dashboard TUI.
// Loads task lists from Store and handles keyboard-driven navigation.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;
use std::sync::Arc;

use crate::store::Store;
use crate::types::{Task, TaskEvent, TaskFilter};

pub struct App {
    pub tasks: Vec<Task>,
    pub events_cache: HashMap<String, Vec<TaskEvent>>,
    pub selected: usize,
    pub detail_mode: bool,
    pub should_quit: bool,
    store: Arc<Store>,
}

impl App {
    pub fn new(store: Arc<Store>) -> Result<Self> {
        let tasks = store.list_tasks(TaskFilter::Today)?;
        Ok(Self {
            tasks,
            events_cache: HashMap::new(),
            selected: 0,
            detail_mode: false,
            should_quit: false,
            store,
        })
    }

    pub fn tick(&mut self) -> Result<()> {
        self.tasks = self.store.list_tasks(TaskFilter::Today)?;
        if self.selected >= self.tasks.len() && !self.tasks.is_empty() {
            self.selected = self.tasks.len() - 1;
        }
        if self.detail_mode {
            self.load_selected_events()?;
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
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

    fn load_selected_events(&mut self) -> Result<()> {
        let Some(task_id) = self.selected_task().map(|task| task.id.as_str().to_string()) else {
            return Ok(());
        };
        let events = self.store.get_events(&task_id)?;
        self.events_cache.insert(task_id, events);
        Ok(())
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
