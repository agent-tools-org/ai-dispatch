// Keyboard input handling for the TUI App.
// Processes key events for list navigation, detail view, and multipane mode.

use super::*;

impl App {
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
            KeyCode::Char('a') => {
                self.show_all = !self.show_all;
                self.reload_tasks()?;
                return Ok(());
            }
            KeyCode::Char('s') => {
                self.stats_mode = !self.stats_mode;
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
