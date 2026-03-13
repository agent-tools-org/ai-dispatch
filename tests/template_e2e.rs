// E2E coverage for prompt template listing and prompt wrapping.
// Verifies `aid config templates` and `aid run --template` through the binary.
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
fn config_templates_lists_installed_templates() {
    let aid_home = TempDir::new().unwrap();
    std::fs::create_dir_all(aid_home.path().join("templates")).unwrap();
    std::fs::write(aid_home.path().join("templates/bug-fix.md"), "# Bug Fix").unwrap();
    let output = aid_cmd_in(aid_home.path()).args(["config", "templates"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Available templates:"));
    assert!(stdout.contains("bug-fix"));
}

#[test]
fn run_wraps_prompt_with_template() {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    std::fs::create_dir_all(aid_home.path().join("templates")).unwrap();
    std::fs::write(aid_home.path().join("templates/bug-fix.md"), "Plan:\n{{prompt}}\nDone.").unwrap();
    write_script(bin_dir.path(), "gemini", "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-p\" ]; then\n    shift\n    printf '%s' \"$1\" > \"$AID_HOME/captured-prompt.txt\"\n    break\n  fi\n  shift\ndone\nprintf '%s' '{\"response\":\"ok\",\"usageMetadata\":{\"totalTokenCount\":7}}'\n");
    let path = format!("{}:{}", bin_dir.path().display(), std::env::var("PATH").unwrap_or_default());
    let output = aid_cmd_in(aid_home.path()).env("PATH", path).args(["run", "gemini", "--template", "bug-fix", "Fix login crash"]).output().unwrap();
    assert!(output.status.success(), "run failed: {}", String::from_utf8_lossy(&output.stderr));
    let captured = std::fs::read_to_string(aid_home.path().join("captured-prompt.txt")).unwrap();
    assert!(captured.contains("Plan:\nFix login crash\nDone."));
}

fn write_script(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    #[cfg(unix)]
    {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
