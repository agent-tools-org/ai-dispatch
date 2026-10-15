// Regression test for task detail navigation and selection stability.
// Covers opening, tab scrolling, and returning to the board.
// Deps: TUI App and the shared app test task fixture.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

#[test]
fn detail_mode_keeps_selection_stable_and_resets_on_escape() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&super::tests::make_task("t-1004", None)).unwrap();
    store.insert_task(&super::tests::make_task("t-1005", None)).unwrap();
    let mut app = App::new(store, super::super::RunOptions::default()).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)).unwrap();
    assert_eq!(app.selected, 0);
    assert_eq!(app.detail_scroll, 0);
    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)).unwrap();
    assert_eq!(app.selected, 0);
    assert_eq!(app.detail_scroll, 0);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).unwrap();
    assert!(!app.detail_mode);
    assert!(matches!(app.detail_tab, DetailTab::Events));
    assert_eq!(app.detail_scroll, 0);
}
