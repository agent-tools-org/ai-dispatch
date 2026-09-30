// Real-process regression tests for bounded CLI probes and identity checks.
// Covers timeout cleanup, capped pipes, exit status, and shared help parsing.
// Deps: temporary shell executables and the production probe runner.

use super::*;
use std::os::unix::fs::PermissionsExt;

fn fake_binary(dir: &Path, body: &str) -> String {
    let path = dir.join("probe");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write probe");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable probe");
    path.to_str().expect("probe path").to_string()
}

#[test]
fn timeout_returns_promptly_and_reaps_the_probe_pid() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("pid");
    let binary = fake_binary(dir.path(), "echo $$ > \"$1\"; exec sleep 60");
    let timeout = Duration::from_millis(200);
    let start = Instant::now();
    let error = run_bounded(&binary, &[pid_file.to_str().expect("pid path")], timeout)
        .expect_err("sleeping probe must time out");
    let elapsed = start.elapsed();
    assert!(error.to_string().contains("timed out"));
    assert!(elapsed >= timeout && elapsed < timeout + Duration::from_secs(1), "{elapsed:?}");
    let pid: i32 = fs::read_to_string(pid_file).expect("probe started").trim().parse().expect("pid");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "probe pid still exists");
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
}

#[test]
fn oversized_help_drains_both_pipes_and_preserves_success() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = fake_binary(dir.path(),
        "printf 'Cursor Agent\\n'; i=0; while [ $i -lt 8192 ]; do printf '0123456789abcdef'; printf 'fedcba9876543210' >&2; i=$((i + 1)); done");
    let start = Instant::now();
    let output = run_bounded(&binary, &["--help"], Duration::from_secs(2)).expect("large help");
    assert!(output.success);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(output.stdout.len(), 64 * 1024);
    assert_eq!(output.stderr.len(), 64 * 1024);
    assert!(output.stdout.starts_with("Cursor Agent\n"));
    assert!(output.stderr.starts_with("fedcba9876543210"));
    assert!(binary_identity_matches(&binary, &["cursor agent"]));
}

#[test]
fn identity_timeout_is_bounded_and_reaps_the_probe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = fake_binary(dir.path(), "echo $$ > \"$0.pid\"; exec sleep 60");
    let start = Instant::now();
    assert!(!binary_identity_matches(&binary, &["cursor agent"]));
    assert!(start.elapsed() < Duration::from_millis(1500));
    let pid: i32 = fs::read_to_string(format!("{binary}.pid"))
        .expect("probe started").trim().parse().expect("pid");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}

#[test]
fn nonzero_exit_preserves_output_but_is_not_a_model_or_identity_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = fake_binary(dir.path(), "echo 'Cursor Agent'; echo 'failure' >&2; exit 7");
    let output = run_bounded(&binary, &[], Duration::from_secs(1)).expect("exit output");
    assert!(!output.success);
    assert_eq!(output.stdout, "Cursor Agent\n");
    assert_eq!(output.stderr, "failure\n");
    assert!(crate::agent::model_validation::run_probe_cmd(&binary, &["models"]).is_none());
    assert!(!binary_identity_matches(&binary, &["cursor agent"]));
}

#[test]
fn missing_probe_is_an_error_and_model_probe_is_unavailable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = dir.path().join("missing");
    let binary = binary.to_str().expect("probe path");
    assert!(run_bounded(binary, &[], Duration::from_secs(1)).is_err());
    assert!(crate::agent::model_validation::run_probe_cmd(binary, &[]).is_none());
}

#[test]
fn flag_definitions_exclude_prefixes_and_prose() {
    for suffix in ["", " value", "\tvalue", "=value", ", -m"] {
        assert!(help_defines_flag(&format!("  --model{suffix}"), "--model"));
    }
    assert!(!help_defines_flag("  --model-fallback value\nUse --model to select", "--model"));
}
