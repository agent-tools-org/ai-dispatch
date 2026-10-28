// transcript tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn finalize_streaming_persists_transcript() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().unwrap());
    let task = crate::types::Task::test_fixture(TaskId("t-pty-transcript".to_string()), TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.full_output.push_str("complete transcript");

    finalize_streaming(
        &crate::agent::codex::CodexAgent,
        &task.id,
        &store,
        &portable_pty::ExitStatus::with_exit_code(0),
        &mut state,
    )
    .unwrap();

    let transcript = std::fs::read_to_string(paths::transcript_path(task.id.as_str())).unwrap();
    assert!(transcript.contains("complete transcript"));
    assert!(transcript.contains("=== AID TASK t-pty-transcript DONE (exit 0) ==="));
}
