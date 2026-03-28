// Transcript-focused `aid run` tests split from run_tests.rs.
// Covers transcript path wiring and transcript-first autosave behavior.
// Deps: parent run test helpers, paths, Store, tempfile.
use super::{auto_save_task_output, make_failed_task, paths};
use crate::store::Store;
use crate::types::TaskStatus;
use tempfile::TempDir;

#[test]
fn transcript_path_helper_works() {
    assert_eq!(
        paths::transcript_path("t-1234"),
        paths::task_dir("t-1234").join("transcript.md")
    );
}

#[test]
fn auto_save_prefers_transcript_over_log() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let transcript = paths::transcript_path("t-transcript-save");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(
        &transcript,
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"transcript output\"}\n",
    )
    .unwrap();
    let log_path = temp.path().join("task.jsonl");
    std::fs::write(
        &log_path,
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"log output\"}\n",
    )
    .unwrap();
    let mut task = make_failed_task("t-transcript-save");
    task.status = TaskStatus::Done;
    task.log_path = Some(log_path.display().to_string());
    store.insert_task(&task).unwrap();

    auto_save_task_output(&store, &task).unwrap();

    let output_path = paths::task_dir(task.id.as_str()).join("output.md");
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        "transcript output"
    );
    assert_eq!(
        store.get_task(task.id.as_str()).unwrap().unwrap().output_path,
        Some(output_path.display().to_string())
    );
}
