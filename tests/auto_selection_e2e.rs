// E2E coverage for `aid run auto`.
// Verifies the stderr selection message and dispatch through a fake agent CLI.
// Deps: compiled `aid` binary, tempfile, and a POSIX shell.

use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

fn aid_cmd_in(aid_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd
}

#[test]
fn auto_run_selects_gemini_and_reports_reason() {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    write_script(
        bin_dir.path(),
        "gemini",
        "#!/bin/sh\nprintf '%s' '{\"response\":\"ok\",\"usageMetadata\":{\"totalTokenCount\":7}}'\n",
    );

    let path = format!(
        "{}:{}",
        bin_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = aid_cmd_in(aid_home.path())
        .env("PATH", path)
        .args(["run", "auto", "Explain the retry flow?"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[aid] Auto-selected: gemini"));
    assert!(stderr.contains("research task"));
}

fn write_script(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    #[cfg(unix)]
    {
        let permissions = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
}
