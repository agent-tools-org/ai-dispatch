// Unit tests for `aid build` outcome evaluation.
// Covers no-target detection, cached success, and event detail wording.
// Deps: parent build_parse module only.

use super::*;
use super::super::build::build_diag::BuildReport;
use std::time::Duration;

fn report(success: bool, stderr: Vec<String>) -> BuildReport {
    BuildReport {
        success,
        command: "cargo check --lib".to_string(),
        elapsed: Duration::from_millis(12),
        diagnostics: Vec::new(),
        stderr_lines: stderr,
        note: None,
    }
}

#[test]
fn zero_units_with_no_targets_never_looks_like_pass() {
    let report = report(
        false,
        vec!["error: no library targets found in package `ai-dispatch`".to_string()],
    );
    let verdict = evaluate_build_run(&report, 0, 101, false);
    assert!(!verdict.success);
    assert_eq!(verdict.exit_code, 101);
    assert!(verdict.digest.contains("failed: no build targets found"));
    assert!(verdict.event_detail.contains("failed: no build targets found"));
    assert!(verdict.event_detail.contains("0 units compiled"));
}

#[test]
fn cached_success_with_zero_units_is_not_failure() {
    let report = BuildReport {
        success: true,
        command: "cargo check".to_string(),
        elapsed: Duration::from_millis(5),
        diagnostics: Vec::new(),
        stderr_lines: Vec::new(),
        note: None,
    };
    let verdict = evaluate_build_run(&report, 0, 0, false);
    assert!(verdict.success);
    assert_eq!(verdict.exit_code, 0);
    assert!(verdict.digest.starts_with("succeeded:"));
    assert!(verdict.event_detail.contains("succeeded:"));
}

#[test]
fn compile_failure_preserves_cargo_exit_code() {
    let report = BuildReport {
        success: false,
        command: "cargo check".to_string(),
        elapsed: Duration::from_millis(50),
        diagnostics: Vec::new(),
        stderr_lines: vec!["error: could not compile `foo`".to_string()],
        note: None,
    };
    let verdict = evaluate_build_run(&report, 4, 101, false);
    assert!(!verdict.success);
    assert_eq!(verdict.exit_code, 101);
    assert!(verdict.digest.starts_with("failed:"));
    assert!(verdict.event_detail.contains("failed:"));
}

#[test]
fn event_detail_uses_succeeded_not_finished() {
    let report = BuildReport {
        success: true,
        command: "cargo check".to_string(),
        elapsed: Duration::from_secs(1),
        diagnostics: Vec::new(),
        stderr_lines: Vec::new(),
        note: None,
    };
    let detail = event_detail_for("cargo check", &report, 42, true);
    assert_eq!(detail, "cargo check succeeded: 0 errors, 0 warnings, 42 units compiled");
}
