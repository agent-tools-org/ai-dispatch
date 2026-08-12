// E2E coverage for human and JSON `aid board` polling behavior.
// Verifies cooldowns retain board data and repeat loops still warn.
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
    assert!(!aid_home.path().join("board-last.txt").exists());
}

#[test]
fn board_cooldown_still_renders_useful_output() {
    let aid_home = TempDir::new().unwrap();
    let first = aid_cmd_in(aid_home.path()).arg("board").output().unwrap();
    assert!(first.status.success());

    let marker = aid_home.path().join("board-last.txt");
    let mut marker_lines: Vec<String> = std::fs::read_to_string(&marker)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    marker_lines[1] = "changed".to_string();
    std::fs::write(&marker, marker_lines.join("\n")).unwrap();

    let second = aid_cmd_in(aid_home.path()).arg("board").output().unwrap();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("No tasks found."));
    assert!(String::from_utf8_lossy(&second.stderr).contains("Board checked"));
}

#[test]
fn board_force_bypasses_cooldown_and_renders_output() {
    let aid_home = TempDir::new().unwrap();
    let first = aid_cmd_in(aid_home.path()).arg("board").output().unwrap();
    assert!(first.status.success());

    let forced = aid_cmd_in(aid_home.path())
        .args(["board", "--force"])
        .output()
        .unwrap();
    assert!(forced.status.success());
    assert!(String::from_utf8_lossy(&forced.stdout).contains("No tasks found."));
}

#[test]
fn board_repeat_loop_renders_output_and_warns() {
    let aid_home = TempDir::new().unwrap();
    let first = aid_cmd_in(aid_home.path()).arg("board").output().unwrap();
    assert!(first.status.success());

    let repeated = aid_cmd_in(aid_home.path()).arg("board").output().unwrap();
    assert_eq!(repeated.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&repeated.stdout).contains("No tasks found."));
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("No changes after"));
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
