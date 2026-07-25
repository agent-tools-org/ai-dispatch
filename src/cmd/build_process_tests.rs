// Unit tests for `aid build` process progress bookkeeping.
// Covers threshold/interval limits, compiled-unit progress text, and Cargo JSON artifact detection.
// Deps: parent build_process module only.

use super::*;

#[test]
fn progress_starts_after_threshold_and_rate_limits() {
    let progress = ProgressConfig::for_tests(100, 50, 2);
    let mut state = ProgressState::new(progress);
    state.next_detail(Duration::from_millis(99), "cargo check", 0);
    assert_eq!(state.emitted, 0);
    state.next_detail(Duration::from_millis(100), "cargo check", 0);
    state.next_detail(Duration::from_millis(120), "cargo check", 0);
    state.next_detail(Duration::from_millis(150), "cargo check", 0);
    state.next_detail(Duration::from_millis(200), "cargo check", 0);
    assert_eq!(state.emitted, 2);
}

#[test]
fn progress_line_includes_compiled_unit_count() {
    let progress = ProgressConfig::for_tests(100, 50, 3);
    let mut state = ProgressState::new(progress);
    let detail = state
        .next_detail(Duration::from_millis(100), "cargo check --all-targets", 187)
        .expect("progress detail");
    assert_eq!(
        detail,
        "cargo check --all-targets still running after 0s, 187 units compiled"
    );
}

#[test]
fn progress_keeps_three_message_cap_with_compiled_units() {
    let progress = ProgressConfig::for_tests(100, 50, 3);
    let mut state = ProgressState::new(progress);
    for idx in 0..6 {
        state.next_detail(Duration::from_millis(100 + idx * 50), "cargo check", idx as usize);
    }
    assert_eq!(state.emitted, 3);
}

#[test]
fn stdout_artifact_lines_increment_compiled_unit_count() {
    assert!(is_compiler_artifact_line(r#"{"reason":"compiler-artifact"}"#));
    assert!(!is_compiler_artifact_line(r#"{"reason":"compiler-message"}"#));
}

#[test]
fn final_detail_includes_compiled_unit_count() {
    let report = BuildReport {
        success: true,
        command: "cargo check".to_string(),
        elapsed: Duration::from_secs(1),
        diagnostics: Vec::new(),
        stderr_lines: Vec::new(),
    };
    assert_eq!(
        finished_detail("cargo check", &report, 187),
        "cargo check finished: 0 errors, 0 warnings, 187 units compiled"
    );
}
