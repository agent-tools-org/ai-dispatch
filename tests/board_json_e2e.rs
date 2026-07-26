// E2E coverage for `aid board --json`.
// Verifies machine-readable board output is not throttled by rapid polling.
// Deps: compiled `aid` binary, tempfile, serde_json.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

#[test]
fn board_json_rapid_calls_emit_parseable_arrays() {
    let aid_home = TempDir::new().unwrap();
    let first = aid_cmd_in(aid_home.path())
        .args(["board", "--json"])
        .output()
        .unwrap();
    assert_json_array_output(&first);

    let second = aid_cmd_in(aid_home.path())
        .args(["board", "--json"])
        .output()
        .unwrap();
    assert_json_array_output(&second);
}

fn assert_json_array_output(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "aid board --json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.is_array(), "expected JSON array, got {value}");
}
