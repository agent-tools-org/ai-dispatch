// Tests for Cursor binary selection and alias detection.
// Exports: none (test module).
// Deps: super::{cursor::CursorAgent, detect_agents, RunOpts}, crate::test_subprocess, tempfile.

use super::{cursor::CursorAgent, detect_agents, Agent, RunOpts};
use crate::test_subprocess;
use crate::types::{AgentKind, EventKind, TaskId};
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
fn build_command_omits_partial_output_flag() {
    let agent = CursorAgent;
    let cmd = agent.build_command("test prompt", &run_opts()).unwrap();
    let args: Vec<_> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(!args.iter().any(|arg| arg == "--stream-partial-output"));
}

#[test]
fn cursor_reasoning_stays_coherent_without_partial_output_flag() {
    let bin_dir = streaming_fake_bin_dir();
    let agent = CursorAgent;
    let task_id = TaskId("t-cursor-stream".to_string());
    let mut cmd = agent.build_command("test prompt", &run_opts()).unwrap();
    cmd.env("PATH", bin_dir.path());

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "fake cursor command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let reasoning: Vec<_> = stdout
        .lines()
        .filter_map(|line| agent.parse_event(&task_id, line))
        .filter(|event| event.event_kind == EventKind::Reasoning)
        .collect();

    assert_eq!(reasoning.len(), 1);
    assert_eq!(reasoning[0].detail, "that omits route artifacts.");
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

fn streaming_fake_bin_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let script = r#"#!/bin/sh
case " $* " in
  *" --stream-partial-output "*)
    printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"that"}]}}'
    printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" omits"}]}}'
    printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" route"}]}}'
    printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":" artifacts."}]}}'
    ;;
  *)
    printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"that omits route artifacts."}]}}'
    ;;
esac
printf '%s\n' '{"type":"result","subtype":"success","result":"that omits route artifacts.","usage":{"inputTokens":1,"outputTokens":4,"cacheReadTokens":0}}'
"#;
    write_executable(&dir.path().join("agent"), script);
    write_executable(&dir.path().join("cursor-agent"), script);
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
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}
