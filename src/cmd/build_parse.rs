// Evaluate `aid build` outcomes: honest zero-unit / no-target detection.
// Exports: BuildVerdict, evaluate_build_run, event_detail_for().
// Deps: build_diag only.

use std::time::Duration;

use super::build::build_diag::{render_digest, BuildReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildVerdict {
    pub(crate) success: bool,
    pub(crate) exit_code: i32,
    pub(crate) digest: String,
    pub(crate) event_detail: String,
}

pub(crate) fn evaluate_build_run(
    report: &BuildReport,
    compiled_units: usize,
    cargo_exit_code: i32,
    include_warnings: bool,
) -> BuildVerdict {
    if let Some(reason) = no_targets_failure(compiled_units, report.success, &report.stderr_lines) {
        return failed_verdict(&reason, report, compiled_units, cargo_exit_code, include_warnings);
    }
    let digest = render_digest(report, include_warnings);
    let success = report.success && !digest.starts_with("failed:");
    BuildVerdict {
        success,
        exit_code: cargo_exit_code,
        event_detail: event_detail_for(&report.command, report, compiled_units, success),
        digest,
    }
}

pub(crate) fn event_detail_for(
    command: &str,
    report: &BuildReport,
    compiled_units: usize,
    success: bool,
) -> String {
    let errors = report.diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = report.diagnostics.len().saturating_sub(errors);
    let outcome = if success { "succeeded" } else { "failed" };
    format!(
        "{command} {outcome}: {errors} errors, {warnings} warnings, {compiled_units} units compiled"
    )
}

fn no_targets_failure(
    compiled_units: usize,
    cargo_success: bool,
    stderr_lines: &[String],
) -> Option<String> {
    if cargo_success || compiled_units > 0 {
        return None;
    }
    if stderr_indicates_no_targets(stderr_lines) {
        return Some("failed: no build targets found".to_string());
    }
    None
}

fn stderr_indicates_no_targets(stderr_lines: &[String]) -> bool {
    stderr_lines.iter().any(|line| {
        let line = line.trim();
        line.contains("no library targets found")
            || line.contains("no bin targets found")
            || line.contains("no targets specified")
            || (line.starts_with("error:") && line.contains("targets found"))
    })
}

fn failed_verdict(
    headline: &str,
    report: &BuildReport,
    compiled_units: usize,
    cargo_exit_code: i32,
    include_warnings: bool,
) -> BuildVerdict {
    let mut lines = vec![format!(
        "{headline}; command: {}; elapsed: {}",
        report.command,
        format_duration(report.elapsed)
    )];
    if let Some(note) = report.note.as_ref() {
        lines.push(note.clone());
    }
    let digest = render_digest(report, include_warnings);
    lines.extend(
        digest
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .map(str::to_string),
    );
    BuildVerdict {
        success: false,
        exit_code: cargo_exit_code,
        event_detail: format!("{headline}, {compiled_units} units compiled"),
        digest: lines.join("\n"),
    }
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }
    format!("{:.1}s", millis as f64 / 1_000.0)
}

#[cfg(test)]
#[path = "build_parse_tests.rs"]
mod tests;
