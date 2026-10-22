// Tests for run output persistence and fallback extraction.
// Deps: parent output helpers and tempfile.
use super::*;
use tempfile::{NamedTempFile, tempdir};

#[test]
fn fill_empty_output_from_log_extracts_full_text_content() {
    let log = NamedTempFile::new().unwrap();
    let output = NamedTempFile::new().unwrap();
    std::fs::write(
        log.path(),
        concat!(
            "{\"type\":\"text\",\"content\":\"first chunk\"}\n",
            "{\"type\":\"text\",\"content\":\"second chunk\"}\n"
        ),
    )
    .unwrap();
    std::fs::write(output.path(), "").unwrap();

    fill_empty_output_from_log(log.path(), Some(output.path()), None).unwrap();

    assert_eq!(
        std::fs::read_to_string(output.path()).unwrap(),
        "second chunk"
    );
}

#[test]
fn fill_empty_output_from_log_creates_missing_output_file() {
    let log = NamedTempFile::new().unwrap();
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("missing-output.txt");
    std::fs::write(
        log.path(),
        "{\"type\":\"text\",\"content\":\"gemini output\"}\n",
    )
    .unwrap();

    fill_empty_output_from_log(log.path(), Some(output_path.as_path()), None).unwrap();

    assert_eq!(
        std::fs::read_to_string(output_path).unwrap(),
        "gemini output"
    );
}

#[test]
fn persist_result_file_resolves_relative_path_from_base_dir() {
    let temp = tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let work_dir = temp.path().join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    std::fs::write(work_dir.join("result.md"), "structured result").unwrap();

    let log_path = crate::support::paths::log_path("t-result");
    let delivery = persist_result_file(
        "t-result",
        Some("result.md"),
        Some(work_dir.to_str().unwrap()),
        &log_path,
        None,
    )
    .unwrap();

    let saved = crate::support::paths::task_dir("t-result").join("result.md");
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "structured result");
    assert_eq!(delivery, ResultDelivery::File);
}

/// The failure that produced a "done" audit with a tool log for a report: the agent
/// never wrote the result file and printed only what it was about to do.
#[test]
fn persist_result_file_flags_narration_only_log_fallback() {
    let temp = tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let log_path = crate::support::paths::log_path("t-narration-result");
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    let narration = "I will run `git diff main..HEAD` to get the list of changes made in the branch.\n\
I will read the generated diff file using the `view_file` tool to inspect the full set of changes.\n\
I will inspect the indexer snapshot file to answer the second question about determinism.\n";
    std::fs::write(&log_path, narration).unwrap();

    let delivery = persist_result_file(
        "t-narration-result",
        Some("result.md"),
        Some("/missing"),
        &log_path,
        None,
    )
    .unwrap();

    assert_eq!(
        delivery,
        ResultDelivery::MissingFile {
            fallback_saved: true
        }
    );
}

#[test]
fn persist_result_file_uses_log_when_source_missing() {
    let temp = tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let log_path = crate::support::paths::log_path("t-log-result");
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    std::fs::write(
        &log_path,
        "{\"type\":\"text\",\"content\":\"stdout report\"}\n",
    )
    .unwrap();

    persist_result_file(
        "t-log-result",
        Some("result.md"),
        Some("/missing"),
        &log_path,
        None,
    )
    .unwrap();

    let saved = crate::support::paths::task_dir("t-log-result").join("result.md");
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "stdout report");
}

#[test]
fn persist_result_file_uses_raw_text_log_when_source_missing() {
    let temp = tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let log_path = crate::support::paths::log_path("t-raw-log-result");
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    std::fs::write(&log_path, "plain text report\nwith final findings").unwrap();

    let delivery = persist_result_file(
        "t-raw-log-result",
        Some("result.md"),
        Some("/missing"),
        &log_path,
        None,
    )
    .unwrap();

    let saved = crate::support::paths::task_dir("t-raw-log-result").join("result.md");
    assert_eq!(
        std::fs::read_to_string(saved).unwrap(),
        "plain text report\nwith final findings"
    );
    assert_eq!(
        delivery,
        ResultDelivery::MissingFile {
            fallback_saved: true
        }
    );
}

#[test]
fn persist_result_file_skips_empty_or_missing_log_when_source_missing() {
    let temp = tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let missing_log = crate::support::paths::log_path("t-missing-log-result");

    persist_result_file(
        "t-missing-log-result",
        Some("result.md"),
        Some("/missing"),
        &missing_log,
        None,
    )
    .unwrap();

    let missing_saved = crate::support::paths::task_dir("t-missing-log-result").join("result.md");
    assert!(!missing_saved.exists());

    let empty_log = crate::support::paths::log_path("t-empty-log-result");
    std::fs::create_dir_all(empty_log.parent().unwrap()).unwrap();
    std::fs::write(&empty_log, "").unwrap();

    persist_result_file(
        "t-empty-log-result",
        Some("result.md"),
        Some("/missing"),
        &empty_log,
        None,
    )
    .unwrap();

    let empty_saved = crate::support::paths::task_dir("t-empty-log-result").join("result.md");
    assert!(!empty_saved.exists());
}
