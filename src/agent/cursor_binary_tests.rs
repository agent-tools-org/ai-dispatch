// Tests for Cursor binary selection and alias detection.
// Exports: none (test module).
// Deps: super::{cursor::CursorAgent, detect_agents, RunOpts}, crate::test_subprocess, tempfile.

use super::{
    cursor::{CursorAgent, CursorBinaryGuard}, detect_agents, ensure_resolved_binary_available, Agent,
    RunOpts,
};
use crate::test_subprocess;
use crate::types::{AgentKind, EventKind, TaskId};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[test]
fn build_command_prefers_agent_binary() {
    let _guard = CursorBinaryGuard::set("agent");
    let agent = CursorAgent;
    let command = agent.build_command("test prompt", &run_opts()).unwrap();
    assert_eq!(command.get_program().to_string_lossy(), "agent");
}

#[test]
fn build_command_ignores_a_foreign_binary_named_agent() {
    let _permit = test_subprocess::acquire();
    let bin_dir = grok_shadowed_bin_dir();
    let output = run_helper(
        "agent::cursor_binary_tests::reports_cursor_binary_for_subprocess",
        &bin_dir,
    );
    assert_eq!(extract_marker(&output, "CURSOR_BINARY="), "cursor-agent");
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
fn detect_agents_rejects_foreign_binary_under_claimable_name() {
    let _permit = test_subprocess::acquire();
    let bin_dir = foreign_agent_only_bin_dir();
    let output = run_helper(
        "agent::cursor_binary_tests::reports_cursor_count_for_subprocess",
        &bin_dir,
    );
    assert_eq!(extract_marker(&output, "CURSOR_COUNT="), "0");
}

#[test]
fn foreign_binary_is_not_reported_or_dispatchable() {
    let _permit = test_subprocess::acquire();
    let bin_dir = foreign_agent_only_bin_dir();
    let output = run_helper(
        "agent::cursor_binary_tests::reports_cursor_availability_for_subprocess",
        &bin_dir,
    );
    assert_eq!(extract_marker(&output, "CURSOR_REPORTED="), "0");
    assert_eq!(extract_marker(&output, "CURSOR_DISPATCHABLE="), "0");
}
#[test]
fn oversized_identity_output_does_not_block_cursor_probe() {
    let _permit = test_subprocess::acquire();
    let bin_dir = oversized_agent_bin_dir();
    let output = run_helper_with_deadline(
        "agent::cursor_binary_tests::reports_cursor_probe_after_oversized_help",
        &bin_dir,
        Duration::from_secs(2),
    );
    assert_eq!(extract_marker(&output, "CURSOR_BINARY="), "cursor-agent");
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
    let _guard = CursorBinaryGuard::set("agent");
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

#[test]
#[ignore]
fn reports_cursor_availability_for_subprocess() {
    let reported = detect_agents().contains(&AgentKind::Cursor);
    let dispatchable = ensure_resolved_binary_available("cursor", "cursor-agent").is_ok();
    println!("CURSOR_REPORTED={}", u8::from(reported));
    println!("CURSOR_DISPATCHABLE={}", u8::from(dispatchable));
}

#[test]
#[ignore]
fn reports_cursor_probe_after_oversized_help() {
    let agent = CursorAgent;
    let cmd = agent.build_command("test prompt", &run_opts()).unwrap();
    let marker = format!("CURSOR_BINARY={}", cmd.get_program().to_string_lossy());
    if let Ok(path) = std::env::var("CURSOR_PROBE_MARKER") {
        fs::write(path, marker).unwrap();
    } else {
        println!("{marker}");
    }
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
    // A real Cursor `agent` names the product in its help; the adapter checks for that
    // before trusting so generic a binary name.
    write_executable(
        &dir.path().join("agent"),
        "#!/bin/sh\necho 'Cursor Agent CLI'\nexit 0\n",
    );
    write_executable(&dir.path().join("cursor-agent"), "#!/bin/sh\nexit 0\n");
    dir
}

/// Same layout, except `agent` is xAI's Grok Build CLI rather than Cursor's.
fn grok_shadowed_bin_dir() -> TempDir {
    let dir = fake_bin_dir();
    write_executable(
        &dir.path().join("agent"),
        "#!/bin/sh\necho 'Grok Build TUI'\necho 'Usage: agent [OPTIONS] [PROMPT] [COMMAND]'\nexit 0\n",
    );
    dir
}

fn foreign_agent_only_bin_dir() -> TempDir {
    let dir = fake_bin_dir();
    fs::remove_file(dir.path().join("cursor-agent")).unwrap();
    write_executable(
        &dir.path().join("agent"),
        "#!/bin/sh\necho 'Grok Build TUI'\necho 'Usage: agent [OPTIONS] [PROMPT] [COMMAND]'\nexit 0\n",
    );
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

fn oversized_agent_bin_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let script = r#"#!/bin/sh
n=1000000
while [ "$n" -gt 0 ]; do printf x; n=$((n - 1)); done
"#;
    write_executable(&dir.path().join("agent"), script);
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

fn run_helper_with_deadline(test_name: &str, bin_dir: &TempDir, deadline: Duration) -> String {
    let marker_path = bin_dir.path().join("probe-marker");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--ignored", "--nocapture"])
        .env("PATH", bin_dir.path())
        .env("CURSOR_PROBE_MARKER", &marker_path)
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "helper test exited unsuccessfully: {status}");
            return fs::read_to_string(marker_path).unwrap_or_default();
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("cursor identity probe exceeded {deadline:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
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
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}
