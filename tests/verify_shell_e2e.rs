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
    assert!(help.contains("a program missing under sh -c fails verification"), "{help}");
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

#[test]
fn auto_verify_uses_project_command_instead_of_rust_check() {
    let harness = Harness::with_git_project();
    std::fs::create_dir(harness.project().join(".aid")).unwrap();
    std::fs::write(harness.project().join(".aid/project.toml"),
        "[project]\nid = 'verify-project'\nverify = 'echo test-target-does-not-compile; exit 101'\n").unwrap();
    // Auto detection would select cargo check; project verification must win before that path.
    std::fs::write(
        harness.project().join("Cargo.toml"),
        "[package]\nname='fixture'\n",
    )
    .unwrap();
    let output = harness
        .aid()
        .args([
            "run",
            settlement::AGENT,
            "settle",
            "--id",
            "t-auto-project",
            "--dir",
            harness.project().to_str().unwrap(),
            "--verify",
            "auto",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(harness.status("t-auto-project").as_deref(), Some("failed"));
    let db = rusqlite::Connection::open(harness.home().join("aid.db")).unwrap();
    let command: String = db
        .query_row(
            "SELECT verify FROM tasks WHERE id = 't-auto-project'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(command, "echo test-target-does-not-compile; exit 101");
}

#[test]
fn background_verify_persists_success_and_failure_evidence() {
    for code in [0, 101] {
        let harness = Harness::with_git_project();
        let id = "t-background-evidence";
        let command = format!("echo durable-verifier-output; exit {code}");
        let output = harness
            .aid()
            .args([
                "run",
                settlement::AGENT,
                "settle",
                "--id",
                id,
                "--bg",
                "--dir",
                harness.project().to_str().unwrap(),
                "--verify",
                &command,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        harness.wait_for(std::time::Duration::from_secs(30), |h| {
            h.status(id)
                .filter(|status| settlement::TERMINAL.contains(&status.as_str()))
        });
        assert_verify_evidence(&harness, id, &command, code);
    }
}

fn assert_verify_evidence(harness: &Harness, id: &str, command: &str, code: i32) {
    let db = rusqlite::Connection::open(harness.home().join("aid.db")).unwrap();
    let metadata: String = db
        .query_row(
            "SELECT metadata FROM events WHERE task_id = ?1
         AND json_extract(metadata, '$.verification') = 'finished'",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["command"], command);
    assert_eq!(metadata["exit_code"], code);
    assert_eq!(metadata["directory"], harness.project().to_str().unwrap());
    assert_eq!(metadata["success"], code == 0);
    assert_eq!(metadata["local_head_before"].as_str().unwrap().len(), 40);
    assert_eq!(metadata["local_head_after"], metadata["local_head_before"]);
    assert!(
        metadata["output_excerpt"]
            .as_str()
            .unwrap()
            .contains("durable-verifier-output")
    );
}
