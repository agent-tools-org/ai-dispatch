// Keyboard input handling for the TUI App.
// Processes key events for list navigation, detail view, and multipane mode.

use super::*;

/// Lines moved by PageUp/PageDown when viewport height is unknown.
const PAGE_SCROLL: usize = 10;

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.search_mode {
            return self.handle_search_key(key);
        }
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char('d') => {
                self.tree_mode = false;
                self.dashboard_mode = !self.dashboard_mode;
                return Ok(());
            }
            KeyCode::Char('a') => {
                self.show_all = !self.show_all;
                self.reload_tasks()?;
                return Ok(());
            }
            KeyCode::Char('s') => {
                self.tree_mode = false;
                self.stats_mode = !self.stats_mode;
                if self.stats_mode {
                    self.refresh_stats()?;
                }
                return Ok(());
            }
            KeyCode::Char('m') => {
                self.tree_mode = false;
                self.multipane_mode = !self.multipane_mode;
                if self.multipane_mode {
                    self.active_pane = 0;
                    self.pane_scroll_offsets.clear();
                    self.active_pane_task_id = self
                        .multipane_tasks()
                        .first()
                        .map(|task| task.id.as_str().to_string());
                }
                return Ok(());
            }
            KeyCode::Char('t') => {
                self.tree_mode = !self.tree_mode;
                if self.tree_mode {
                    self.dashboard_mode = false;
                    self.stats_mode = false;
                    self.multipane_mode = false;
                    self.detail_mode = false;
                    self.select_first_task_if_header();
                }
                return Ok(());
            }
            _ => {}
        }
        if self.multipane_mode {
            return self.handle_multipane_key(key);
        }
        if self.stats_mode {
            if key.code == KeyCode::Char('v') {
                self.legacy_stats_view = !self.legacy_stats_view;
                return Ok(());
            }
            return self.handle_stats_key(key);
        }
        if self.detail_mode {
            return self.handle_detail_key(key);
        }
        if self.tree_mode {
            return self.handle_board_navigation(key);
        }
        self.handle_board_navigation(key)
    }

    fn handle_stats_key(&mut self, key: KeyEvent) -> Result<()> {
        let next_range = match key.code {
            KeyCode::Left | KeyCode::Char('h') => Some(self.stats_range.previous()),
            KeyCode::Right | KeyCode::Char('l') => Some(self.stats_range.next()),
            KeyCode::Tab => Some(self.stats_range.next()),
            KeyCode::BackTab => Some(self.stats_range.previous()),
            _ => None,
        };
        if let Some(range) = next_range {
            self.set_stats_range(range)?;
        }
        Ok(())
    }

    fn handle_multipane_key(&mut self, key: KeyEvent) -> Result<()> {
        self.reconcile_active_pane();
        match key.code {
            KeyCode::Tab => {
                let pane_count = self.pane_count();
                if pane_count > 0 {
                    self.set_active_pane((self.active_pane + 1) % pane_count);
                }
            }
            KeyCode::BackTab => {
                let pane_count = self.pane_count();
                if pane_count > 0 {
                    let index = if self.active_pane == 0 {
                        pane_count - 1
                    } else {
                        self.active_pane - 1
                    };
                    self.set_active_pane(index);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(task_id) = self.current_pane_task_id() {
                    let offset = self.pane_scroll_offsets.entry(task_id).or_default();
                    *offset = offset.saturating_sub(1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_pane_by(self.active_pane, 1);
            }
            KeyCode::PageDown => {
                if let Some(task_id) = self.current_pane_task_id() {
                    let offset = self.pane_scroll_offsets.entry(task_id).or_default();
                    *offset = offset.saturating_sub(PAGE_SCROLL);
                }
            }
            KeyCode::PageUp => {
                self.scroll_pane_by(self.active_pane, PAGE_SCROLL);
            }
            KeyCode::Home => {
                // Home = oldest end of the stream (max offset).
                let max = self.pane_max_scroll(self.active_pane);
                if let Some(task_id) = self.current_pane_task_id() {
                    self.pane_scroll_offsets.insert(task_id, max);
                }
            }
            KeyCode::End => {
                // End = live tail.
                if let Some(task_id) = self.current_pane_task_id() {
                    self.pane_scroll_offsets.insert(task_id, 0);
                }
            }
            KeyCode::Enter => {
                if let Some(task_id) = self.current_pane_task_id()
                    && let Some(idx) = self.tasks.iter().position(|t| t.id.as_str() == task_id)
                {
                    self.selected = idx;
                    self.multipane_mode = false;
                    self.enter_detail_mode()?;
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
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
                self.clamp_detail_scroll();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(PAGE_SCROLL);
                self.clamp_detail_scroll();
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(PAGE_SCROLL);
            }
            KeyCode::Home => {
                self.detail_scroll = 0;
            }
            KeyCode::End => {
                self.detail_scroll = self.detail_max_scroll();
            }
            KeyCode::Esc => {
                self.detail_mode = false;
                self.reset_detail_state();
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn clamp_detail_scroll(&mut self) {
        let max = self.detail_max_scroll();
        if self.detail_scroll > max {
            self.detail_scroll = max;
        }
    }

    fn detail_max_scroll(&self) -> usize {
        self.detail_content_lines().saturating_sub(1)
    }

    fn detail_content_lines(&self) -> usize {
        match self.detail_tab {
            DetailTab::Events => self.selected_events().len().max(1),
            DetailTab::Prompt => self
                .selected_task()
                .map(prompt_line_count)
                .unwrap_or(1)
                .max(1),
            DetailTab::Output => self
                .selected_task()
                .map(|task| crate::task_view::read_output(task).lines().count())
                .unwrap_or(1)
                .max(1),
        }
    }

    pub(super) fn enter_detail_mode(&mut self) -> Result<()> {
        self.tree_mode = false;
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

}

fn prompt_line_count(task: &crate::types::Task) -> usize {
    if let Some(resolved) = &task.resolved_prompt {
        // Mirrors ui_helpers::prompt_text section headers + blank line.
        1 + task.prompt.lines().count().max(1) + 2 + 1 + resolved.lines().count().max(1)
    } else {
        task.prompt.lines().count().max(1)
    }
}
