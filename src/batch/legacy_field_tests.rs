// Regression tests for renamed batch TOML fields.
// Keeps migration-specific coverage separate from the main parser test file.

use super::parse_batch_file;
use std::io::Write;
use tempfile::NamedTempFile;

fn write_temp(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn renamed_defaults_timeout_has_clear_error() {
    let path = write_temp(
        "[defaults]\ntimeout = 30\n\n[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\n",
    );
    let err = parse_batch_file(path.path()).unwrap_err().to_string();
    assert!(err.contains("timeout"));
    assert!(err.contains("max_duration_mins"));
    assert!(err.contains("[defaults]"));
}

#[test]
fn renamed_task_timeout_has_clear_error() {
    let path = write_temp(
        "[[tasks]]\nname = \"build\"\nagent = \"codex\"\nprompt = \"test\"\ntimeout = 30\n",
    );
    let err = parse_batch_file(path.path()).unwrap_err().to_string();
    assert!(err.contains("timeout"));
    assert!(err.contains("max_duration_mins"));
    assert!(err.contains("task build"));
}

#[test]
fn renamed_task_alias_timeout_has_clear_error() {
    let path = write_temp(
        "[[task]]\nname = \"build\"\nagent = \"codex\"\nprompt = \"test\"\ntimeout = 30\n",
    );
    let err = parse_batch_file(path.path()).unwrap_err().to_string();
    assert!(err.contains("timeout"));
    assert!(err.contains("max_duration_mins"));
    assert!(err.contains("task build"));
}
