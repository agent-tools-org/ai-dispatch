// Tests for Cursor binary selection and alias detection.
// Exports: none (test module).
// Deps: super::{cursor::CursorAgent, detect_agents, RunOpts}, crate::test_subprocess, tempfile.

use super::{cursor::CursorAgent, detect_agents, Agent, RunOpts};
use crate::test_subprocess;
use crate::types::AgentKind;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn build_command_prefers_agent_binary() {
    let _permit = test_subprocess::acquire();
    let bin_dir = fake_bin_dir();
    let output = run_helper(
        "agent::cursor_binary_tests::reports_cursor_binary_for_subprocess",
        &bin_dir,
    );
    assert_eq!(extract_marker(&output, "CURSOR_BINARY="), "agent");
}

#[test]
fn detect_agents_deduplicates_cursor_aliases() {
    let _permit = test_subprocess::acquire();
    let bin_dir = fake_bin_dir();
    let output = run_helper(
        "agent::cursor_binary_tests::reports_cursor_count_for_subprocess",
        &bin_dir,
    );
    assert_eq!(extract_marker(&output, "CURSOR_COUNT="), "1");
}

#[test]
#[ignore]
fn reports_cursor_binary_for_subprocess() {
    let agent = CursorAgent;
    let cmd = agent.build_command("test prompt", &run_opts()).unwrap();
    println!("CURSOR_BINARY={}", cmd.get_program().to_string_lossy());
}

#[test]
#[ignore]
fn reports_cursor_count_for_subprocess() {
    let count = detect_agents()
        .into_iter()
        .filter(|kind| *kind == AgentKind::Cursor)
        .count();
    println!("CURSOR_COUNT={count}");
}

fn fake_bin_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let which = String::from_utf8(
        Command::new("which")
            .arg("which")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    write_executable(
        &dir.path().join("which"),
        &format!("#!/bin/sh\nexec {} \"$@\"\n", which.trim()),
    );
    write_executable(&dir.path().join("agent"), "#!/bin/sh\nexit 0\n");
    write_executable(&dir.path().join("cursor-agent"), "#!/bin/sh\nexit 0\n");
    dir
}

fn write_executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn run_helper(test_name: &str, bin_dir: &TempDir) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--ignored", "--nocapture"])
        .env("PATH", bin_dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "helper test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn extract_marker<'a>(output: &'a str, prefix: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing marker {prefix} in output: {output}"))
}

fn run_opts() -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}
