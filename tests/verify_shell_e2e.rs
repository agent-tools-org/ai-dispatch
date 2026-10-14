// Verify shell semantics through foreground aid run and task settlement.
// Uses the existing custom shell-agent harness and compiled aid binary.
#![cfg(unix)]

#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "common/settlement.rs"]
mod settlement;

use settlement::Harness;

fn verify_command(command: &str, success: bool) {
    let harness = Harness::new();
    let id = "t-verify-shell";
    let output = harness
        .aid()
        .args([
            "run",
            settlement::AGENT,
            "settle the task",
            "--id",
            id,
            "--dir",
            harness.project().to_str().unwrap(),
            "--verify",
            command,
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(if success { 0 } else { 1 }),
        "command={command}; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        harness.status(id).as_deref(),
        Some(if success { "done" } else { "failed" })
    );
}

#[test]
fn verify_shell_compound_command_fails() {
    verify_command("true && false", false);
}

#[test]
fn verify_shell_pipeline_passes() {
    verify_command("printf 'a b' | grep -q 'a b'", true);
}

#[test]
fn verify_shell_quoted_argument_passes() {
    verify_command("test 'a b' = 'a b'", true);
}

#[test]
fn run_help_describes_verify_shell_semantics() {
    let harness = Harness::new();
    let output = harness.aid().args(["run", "--help"]).output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("uses aid's build runner"), "{help}");
    assert!(help.contains("--message-format=json"), "{help}");
    assert!(help.contains("sh -c in the task directory"), "{help}");
    assert!(
        help.contains("bare --verify auto-detects"),
        "{help}"
    );
    assert!(help.contains("an absent flag uses the project default"), "{help}");
    assert!(help.contains("a missing program fails verification"), "{help}");
}

#[test]
fn cargo_prefixed_compound_verify_runs_second_command() {
    verify_command("cargo check --help && false", false);
}

#[test]
fn checker_exit_127_is_ordinary_failure() {
    verify_command("sh -c 'exit 127'", false);
}

#[test]
fn checker_exit_126_is_ordinary_failure() {
    verify_command("sh -c 'exit 126'", false);
}

#[test]
fn stdout_shell_like_diagnostic_is_ordinary_failure() {
    verify_command("printf 'sh: 1: checker: not found\\n'; exit 127", false);
}

#[test]
fn checker_stderr_is_ordinary_failure() {
    verify_command("printf 'checker: input not found\\n' >&2; exit 127", false);
}

#[test]
fn cargo_version_compound_verify_runs_second_command() {
    verify_command("cargo --version && false", false);
}

#[test]
fn missing_verify_program_fails_task() {
    verify_command("missing-aid-verify-bin", false);
}

#[test]
fn checker_shell_like_stderr_cannot_override_exit_status() {
    for (code, message) in [(126, "Permission denied"), (127, "not found")] {
        let command = format!("printf 'sh: 1: checker: {message}\\n' >&2; exit {code}");
        verify_command(&command, false);
    }
}
