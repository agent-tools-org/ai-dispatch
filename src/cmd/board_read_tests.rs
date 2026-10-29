// Output and payload-limit regressions for board and its shared stream query.
// Compares text/JSON output against the former list/filter/truncate path.

use super::*;


fn seed(store: &Store) {
    for i in 0..120 {
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, resolved_prompt, status, created_at, duration_ms)
             VALUES (?1, 'cursor', ?2, ?2, 'done', '2026-10-01T00:00:00Z', 1000)",
                rusqlite::params![format!("t-{i}"), format!("prompt-{i}").repeat(1000)],
            )
            .unwrap();
    }
}

#[test]
fn board_and_stream_selection_keep_text_json_and_showing_count() {
    let (_home, store) = crate::support::test_env::TestHome::new().store();
    seed(&store);
    for limit in [None, Some(0), Some(3), Some(500)] {
        let mut expected = store.list_tasks(TaskFilter::All).unwrap();
        let expected_notice = apply_limit(&mut expected, limit, false, false, false, None);
        let (actual, project, notice) =
            board_filter::load_board_tasks(&store, false, false, false, None, true, limit).unwrap();
        assert_eq!(project, None);
        assert_eq!(notice, expected_notice);
        for json in [false, true] {
            let mut before = Vec::new();
            let mut after = Vec::new();
            write_board_output(
                &mut before,
                &store,
                &expected,
                None,
                None,
                true,
                expected_notice,
                json,
            )
            .unwrap();
            write_board_output(&mut after, &store, &actual, None, None, true, notice, json).unwrap();
            assert_eq!(after, before);
        }
    }
}

#[test]
fn board_does_not_decode_hidden_task_payloads() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    store
        .db()
        .execute("UPDATE tasks SET prompt = X'ff' WHERE id = 't-0'", [])
        .unwrap();
    assert!(store.list_tasks(TaskFilter::All).is_err());
    let (tasks, _, notice) =
        board_filter::load_board_tasks(&store, false, false, false, None, true, Some(2)).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(notice, Some(TruncationNotice { shown: 2, total: 120 }));
}
