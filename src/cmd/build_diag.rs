// Cargo JSON diagnostic parsing and digest rendering for `aid build`.
// Exports: DiagnosticCollector, BuildReport, render_digest().
// Deps: serde_json, std collections/time.

use std::collections::HashMap;
use std::time::Duration;

const MAX_DIGEST_LINES: usize = 50;

#[derive(Debug, serde::Deserialize)]
struct CompilerMessageReason {
    message: Message,
}

#[derive(Debug, serde::Deserialize)]
struct Message {
    level: String,
    message: String,
    #[serde(default)]
    spans: Vec<Span>,
}

#[derive(Debug, serde::Deserialize)]
struct Span {
    file_name: String,
    line_start: usize,
    is_primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DiagnosticLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub(crate) level: DiagnosticLevel,
    pub(crate) file_name: String,
    pub(crate) line: usize,
    pub(crate) message: String,
    occurrences: usize,
}

#[derive(Debug, Default)]
pub(crate) struct DiagnosticCollector {
    counts: HashMap<DiagnosticKey, usize>,
    ordered: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiagnosticKey {
    level: DiagnosticLevel,
    file_name: String,
    line: usize,
    message: String,
}

#[derive(Debug)]
pub(crate) struct BuildReport {
    pub(crate) success: bool,
    pub(crate) command: String,
    pub(crate) elapsed: Duration,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) stderr_lines: Vec<String>,
    pub(crate) note: Option<String>,
}

impl DiagnosticCollector {
    pub(crate) fn push_json_line(&mut self, line: &str) -> Option<Diagnostic> {
        let diagnostic = parse_diagnostic(line)?;
        let key = diagnostic.key();
        if let Some(count) = self.counts.get_mut(&key) {
            *count += 1;
            return None;
        }
        self.counts.insert(key, 1);
        self.ordered.push(diagnostic.clone());
        Some(diagnostic)
    }

    pub(crate) fn into_diagnostics(mut self) -> Vec<Diagnostic> {
        self.ordered
            .into_iter()
            .map(|mut diagnostic| {
                diagnostic.occurrences = self.counts.remove(&diagnostic.key()).unwrap_or(1);
                diagnostic
            })
            .collect()
    }
}

impl Diagnostic {
    pub(crate) fn event_detail(&self) -> String {
        format!(
            "cargo {}: {} at {}:{}",
            self.level.as_str(),
            self.message,
            self.file_name,
            self.line
        )
    }

    pub(crate) fn is_error(&self) -> bool {
        self.level == DiagnosticLevel::Error
    }

    fn key(&self) -> DiagnosticKey {
        DiagnosticKey {
            level: self.level,
            file_name: self.file_name.clone(),
            line: self.line,
            message: self.message.clone(),
        }
    }
}

pub(crate) fn render_digest(report: &BuildReport, include_warnings: bool) -> String {
    let errors = count_level(&report.diagnostics, DiagnosticLevel::Error);
    let warnings = count_level(&report.diagnostics, DiagnosticLevel::Warning);
    let outcome = if report.success && errors == 0 { "succeeded" } else { "failed" };
    let mut lines = vec![format!(
        "{outcome}: {errors} errors, {warnings} warnings; command: {}; elapsed: {}",
        report.command,
        format_duration(report.elapsed)
    )];
    if let Some(note) = report.note.as_ref() {
        lines.push(note.clone());
    }
    lines.extend(render_diagnostic_lines(report, include_warnings));
    cap_digest_lines(lines).join("\n")
}

fn render_diagnostic_lines(report: &BuildReport, include_warnings: bool) -> Vec<String> {
    let mut lines = report
        .diagnostics
        .iter()
        .filter(|diagnostic| include_warnings || diagnostic.level == DiagnosticLevel::Error)
        .map(render_diagnostic)
        .collect::<Vec<_>>();
    if lines.is_empty() && !report.success {
        lines.extend(report.stderr_lines.iter().filter_map(|line| render_stderr_line(line)));
    }
    lines
}

fn render_diagnostic(diagnostic: &Diagnostic) -> String {
    format!(
        "{}: {}:{}: {}{}",
        diagnostic.level.as_str(),
        diagnostic.file_name,
        diagnostic.line,
        diagnostic.message,
        occurrence_suffix(diagnostic.occurrences)
    )
}

fn occurrence_suffix(occurrences: usize) -> String {
    if occurrences > 1 {
        return format!(" (x{occurrences})");
    }
    String::new()
}

fn render_stderr_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || is_cargo_progress_line(trimmed) {
        None
    } else {
        Some(format!("error: cargo:0: {trimmed}"))
    }
}

fn is_cargo_progress_line(line: &str) -> bool {
    [
        "Blocking",
        "Checking",
        "Compiling",
        "Finished",
        "Fresh",
        "Running",
        "Waiting",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn cap_digest_lines(mut lines: Vec<String>) -> Vec<String> {
    if lines.len() <= MAX_DIGEST_LINES {
        return lines;
    }
    let suppressed = lines.len() - MAX_DIGEST_LINES + 1;
    lines.truncate(MAX_DIGEST_LINES - 1);
    lines.push(format!("... {suppressed} more diagnostics suppressed"));
    lines
}

fn parse_diagnostic(line: &str) -> Option<Diagnostic> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let reason = value.get("reason").and_then(|reason| reason.as_str());
    if reason != Some("compiler-message") {
        return None;
    }
    let message = serde_json::from_value::<CompilerMessageReason>(value).ok()?.message;
    let level = DiagnosticLevel::from_cargo_level(&message.level)?;
    let span = message.spans.iter().find(|span| span.is_primary).or_else(|| message.spans.first());
    let (file_name, line) = span
        .map(|span| (span.file_name.clone(), span.line_start))
        .unwrap_or_else(|| ("cargo".to_string(), 0));
    Some(Diagnostic {
        level,
        file_name,
        line,
        message: normalize_message(&message.message),
        occurrences: 1,
    })
}

fn normalize_message(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn count_level(diagnostics: &[Diagnostic], level: DiagnosticLevel) -> usize {
    diagnostics.iter().filter(|diagnostic| diagnostic.level == level).count()
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }
    format!("{:.1}s", millis as f64 / 1_000.0)
}

impl DiagnosticLevel {
    fn from_cargo_level(level: &str) -> Option<Self> {
        match level {
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[cfg(test)]
#[path = "build_diag_tests.rs"]
mod tests;
