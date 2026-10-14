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
    assert!(help.contains("sh -c in the task directory"), "{help}");
    assert!(
        help.contains("bare --verify uses the project default"),
        "{help}"
    );
}
