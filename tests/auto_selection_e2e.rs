// E2E coverage: `aid run auto` is a hard error naming `aid advise`.
// Verifies the removed-agent message; scoring stays available via advise.
// Deps: compiled `aid` binary, tempfile.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

#[test]
fn auto_run_errors_with_advise_replacement() {
    let aid_home = TempDir::new().unwrap();
    let output = aid_cmd_in(aid_home.path())
        .args(["run", "auto", "x"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure, got success: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("aid advise"),
        "expected advise replacement hint, got:\n{stderr}"
    );
    assert!(
        stderr.contains("auto") && stderr.contains("removed"),
        "expected removed-auto message, got:\n{stderr}"
    );
}
