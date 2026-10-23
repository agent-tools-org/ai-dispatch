// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn detail_mode_cycles_tabs_and_resets_scroll() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-1003", None);
    // Multi-line prompt so j can advance scroll past 0 under clamp.
    task.prompt = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    store.insert_task(&task).unwrap();
    let mut app = {
        let mut app = App::empty(
            store,
            super::super::super::super::RunOptions {
                task_id: None,
                group: None,
            },
        );
        app.reload_tasks().unwrap();
        app
    };

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
