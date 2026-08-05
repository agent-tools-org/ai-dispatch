// Unit tests for `aid build` diagnostic deduplication and digest rendering.
// Covers unique counts, occurrence suffixes, warning visibility, and digest caps.
// Deps: parent build_diag module only.

use super::*;

fn compiler_message(level: &str, file: &str, line: usize, message: &str) -> String {
    format!(
        r#"{{"reason":"compiler-message","message":{{"level":"{level}","message":"{message}","spans":[{{"file_name":"{file}","line_start":{line},"is_primary":true}}]}}}}"#
    )
}

#[test]
fn collector_deduplicates_matching_messages() {
    let line = compiler_message("error", "src/main.rs", 9, "missing semicolon");
    let mut collector = DiagnosticCollector::default();
    assert!(collector.push_json_line(&line).is_some());
    assert!(collector.push_json_line(&line).is_none());
    assert_eq!(collector.into_diagnostics().len(), 1);
}

#[test]
fn digest_counts_warnings_without_rendering_by_default() {
    let report = BuildReport {
        success: false,
        command: "cargo check".to_string(),
        elapsed: Duration::from_millis(120),
        diagnostics: vec![
            parse_diagnostic(&compiler_message("error", "src/lib.rs", 2, "bad type")).expect("error diagnostic"),
            parse_diagnostic(&compiler_message("warning", "src/lib.rs", 4, "unused")).expect("warning diagnostic"),
        ],
        stderr_lines: Vec::new(),
        note: None,
    };
    let digest = render_digest(&report, false);
    assert!(digest.contains("failed: 1 errors, 1 warnings"));
    assert!(digest.contains("error: src/lib.rs:2: bad type"));
    assert!(!digest.contains("warning: src/lib.rs:4: unused"));
}

#[test]
fn digest_renders_warnings_when_requested() {
    let report = BuildReport {
        success: true,
        command: "cargo clippy".to_string(),
        elapsed: Duration::from_secs(1),
        diagnostics: vec![
            parse_diagnostic(&compiler_message("warning", "src/lib.rs", 4, "unused")).expect("warning diagnostic"),
        ],
        stderr_lines: Vec::new(),
        note: None,
    };
    assert!(render_digest(&report, true).contains("warning: src/lib.rs:4: unused"));
}

#[test]
fn digest_includes_fallback_note_after_status_line() {
    let report = BuildReport {
        success: true,
        command: "cargo check".to_string(),
        elapsed: Duration::from_millis(200),
        diagnostics: Vec::new(),
        stderr_lines: Vec::new(),
        note: Some("note: CARGO_TARGET_DIR unwritable; fell back from /shared to /local/target".into()),
    };
    let digest = render_digest(&report, false);
    let lines: Vec<_> = digest.lines().collect();
    assert!(lines[0].starts_with("succeeded:"));
    assert_eq!(
        lines[1],
        "note: CARGO_TARGET_DIR unwritable; fell back from /shared to /local/target"
    );
}

#[test]
fn diagnostic_seen_once_has_no_occurrence_suffix() {
    let line = compiler_message("error", "src/lib.rs", 2, "bad type");
    let mut collector = DiagnosticCollector::default();
    collector.push_json_line(&line);
    let report = report_with_diagnostics(collector.into_diagnostics());
    let digest = render_digest(&report, false);
    assert!(digest.contains("error: src/lib.rs:2: bad type"));
    assert!(!digest.contains("(x1)"));
}

#[test]
fn diagnostic_seen_multiple_times_gets_occurrence_suffix() {
    let line = compiler_message("error", "src/lib.rs", 2, "bad type");
    let mut collector = DiagnosticCollector::default();
    collector.push_json_line(&line);
    collector.push_json_line(&line);
    collector.push_json_line(&line);
    let report = report_with_diagnostics(collector.into_diagnostics());
    let digest = render_digest(&report, false);
    assert!(digest.contains("error: src/lib.rs:2: bad type (x3)"));
}

#[test]
fn occurrence_counts_do_not_change_unique_status_counts() {
    let error = compiler_message("error", "src/lib.rs", 2, "bad type");
    let warning = compiler_message("warning", "src/lib.rs", 4, "unused");
    let mut collector = DiagnosticCollector::default();
    collector.push_json_line(&error);
    collector.push_json_line(&error);
    collector.push_json_line(&warning);
    collector.push_json_line(&warning);
    let report = report_with_diagnostics(collector.into_diagnostics());
    let digest = render_digest(&report, true);
    assert!(digest.contains("failed: 1 errors, 1 warnings"));
    assert!(digest.contains("error: src/lib.rs:2: bad type (x2)"));
    assert!(digest.contains("warning: src/lib.rs:4: unused (x2)"));
}

#[test]
fn digest_marks_suppressed_diagnostics() {
    let diagnostics = (0..60)
        .map(|idx| {
            parse_diagnostic(&compiler_message("error", "src/lib.rs", idx + 1, "bad type")).expect("error diagnostic")
        })
        .collect();
    let report = BuildReport {
        success: false,
        command: "cargo check".to_string(),
        elapsed: Duration::from_secs(1),
        diagnostics,
        stderr_lines: Vec::new(),
        note: None,
    };
    let digest = render_digest(&report, true);
    assert_eq!(digest.lines().count(), MAX_DIGEST_LINES);
    assert!(digest.contains("... 12 more diagnostics suppressed"));
}

fn report_with_diagnostics(diagnostics: Vec<Diagnostic>) -> BuildReport {
    BuildReport {
        success: false,
        command: "cargo check".to_string(),
        elapsed: Duration::from_secs(1),
        diagnostics,
        stderr_lines: Vec::new(),
        note: None,
    }
}
