// Keyboard input handling for the TUI App.
// Processes key events for list navigation, detail view, and multipane mode.

use super::*;

/// Lines moved by PageUp/PageDown when viewport height is unknown.
const PAGE_SCROLL: usize = 10;

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
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
                return Ok(());
            }
            KeyCode::Char('m') => {
                self.tree_mode = false;
                self.multipane_mode = !self.multipane_mode;
                if self.multipane_mode {
                    self.active_pane = 0;
                    self.pane_scroll_offsets.clear();
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
        if self.tree_mode {
            return self.handle_tree_key(key);
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Enter => self.enter_detail_mode()?,
            _ => {}
        }
        Ok(())
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
            KeyCode::Down | KeyCode::Char('j')
                if self.active_pane < self.pane_scroll_offsets.len() =>
            {
                let offset = &mut self.pane_scroll_offsets[self.active_pane];
                *offset = offset.saturating_sub(1);
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.active_pane < self.pane_scroll_offsets.len() =>
            {
                self.scroll_pane_by(self.active_pane, 1);
            }
            KeyCode::PageDown if self.active_pane < self.pane_scroll_offsets.len() => {
                let offset = &mut self.pane_scroll_offsets[self.active_pane];
                *offset = offset.saturating_sub(PAGE_SCROLL);
            }
            KeyCode::PageUp if self.active_pane < self.pane_scroll_offsets.len() => {
                self.scroll_pane_by(self.active_pane, PAGE_SCROLL);
            }
            KeyCode::Home if self.active_pane < self.pane_scroll_offsets.len() => {
                // Home = oldest end of the stream (max offset).
                let max = self.pane_max_scroll(self.active_pane);
                self.pane_scroll_offsets[self.active_pane] = max;
            }
            KeyCode::End if self.active_pane < self.pane_scroll_offsets.len() => {
                // End = live tail.
                self.pane_scroll_offsets[self.active_pane] = 0;
            }
            KeyCode::Enter => {
                let tasks = self.multipane_tasks();
                if let Some(task) = tasks.get(self.active_pane)
                    && let Some(idx) = self.tasks.iter().position(|t| t.id == task.id)
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

    pub(super) fn clamp_all_pane_scrolls(&mut self) {
        for i in 0..self.pane_scroll_offsets.len() {
            let max = self.pane_max_scroll(i);
            if self.pane_scroll_offsets[i] > max {
                self.pane_scroll_offsets[i] = max;
            }
        }
    }

    fn scroll_pane_by(&mut self, pane: usize, delta: usize) {
        let max = self.pane_max_scroll(pane);
        let offset = &mut self.pane_scroll_offsets[pane];
        *offset = (*offset).saturating_add(delta).min(max);
    }

    fn pane_max_scroll(&self, pane: usize) -> usize {
        let tasks = self.multipane_tasks();
        let Some(task) = tasks.get(pane) else {
            return 0;
        };
        let n = self
            .events_cache
            .get(task.id.as_str())
            .map(|events| events.len())
            .unwrap_or(0);
        n.saturating_sub(1)
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

    fn handle_tree_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') if self.tree_node_count > 0 => {
                self.tree_selected = (self.tree_selected + 1) % self.tree_node_count;
            }
            KeyCode::Up | KeyCode::Char('k') if self.tree_node_count > 0 => {
                self.tree_selected = if self.tree_selected == 0 {
                    self.tree_node_count - 1
                } else {
                    self.tree_selected - 1
                };
            }
            KeyCode::Enter => {
                // Map tree_selected back to app.tasks index for detail view
                let nodes = crate::tui::tree_data::build_task_tree_with_creators(&self.tasks, &self.wg_creators);
                if let Some(node) = nodes.get(self.tree_selected)
                    && let Some(idx) = self.tasks.iter().position(|t| t.id == node.task.id) {
                        self.selected = idx;
                        self.tree_mode = false;
                        self.enter_detail_mode()?;
                    }
            }
            KeyCode::Esc => {
                self.tree_mode = false;
            }
            _ => {}
        }
        Ok(())
    }

    fn enter_detail_mode(&mut self) -> Result<()> {
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

fn prompt_line_count(task: &crate::types::Task) -> usize {
    if let Some(resolved) = &task.resolved_prompt {
        // Mirrors ui_helpers::prompt_text section headers + blank line.
        1 + task.prompt.lines().count().max(1) + 2 + 1 + resolved.lines().count().max(1)
    } else {
        task.prompt.lines().count().max(1)
    }
}
