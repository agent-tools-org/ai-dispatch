// TUI entrypoint for the interactive task dashboard.
// Boots ratatui+crossterm, runs the app loop, and restores the terminal on exit.

pub mod app;
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

pub fn run(store: &Arc<Store>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, app::App::new(store.clone())?);
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
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key)?;
            }
        }
        app.tick()?;
        if app.should_quit {
            return Ok(());
        }
    }
}
