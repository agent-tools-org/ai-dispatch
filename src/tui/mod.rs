// TUI entrypoint for the interactive task dashboard.
// Boots ratatui+crossterm, runs the app loop, and restores the terminal on exit.

pub mod app;
pub mod dashboard;
pub mod metrics;
pub mod multipane;
pub mod ui;

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{
    disable_raw_mode,
    enable_raw_mode,
    EnterAlternateScreen,
    LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;

use crate::store::Store;

#[derive(Debug, Default)]
pub struct RunOptions {
    pub task_id: Option<String>,
    pub group: Option<String>,
}

pub fn run(store: &Arc<Store>, options: RunOptions) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, app::App::new(store.clone(), options)?);
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    mut app: app::App,
) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key)?;
        }
        app.tick()?;
        if app.should_quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn toggles_dashboard_mode_with_d_key() {
        let store = Arc::new(Store::open_memory().unwrap());
        let mut app = app::App::new(store, RunOptions::default()).unwrap();

        assert!(!app.dashboard_mode);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)).unwrap();
        assert!(app.dashboard_mode);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)).unwrap();
        assert!(!app.dashboard_mode);
    }
}
