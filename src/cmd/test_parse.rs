// Parse cargo libtest plain stdout and enforce aid test guarantees.
// Exports: TestHarnessSummary, parse_libtest_lines, evaluate_test_run.
// Deps: std only.

use std::time::Duration;

const MAX_DIGEST_LINES: usize = 50;
const MAX_NAMED_TESTS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutedTest {
    pub(crate) name: String,
    pub(crate) status: TestStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestStatus {
    Ok,
    Failed,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SuiteResult {
    pub(crate) running: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) ignored: usize,
    pub(crate) filtered_out: usize,
    pub(crate) ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TestHarnessSummary {
    pub(crate) suites: Vec<SuiteResult>,
    pub(crate) executed: Vec<ExecutedTest>,
    pub(crate) failure_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestVerdict {
    pub(crate) success: bool,
    pub(crate) exit_code: i32,
    pub(crate) digest: String,
}

pub(crate) fn parse_libtest_lines(lines: &[String]) -> TestHarnessSummary {
    let mut summary = TestHarnessSummary::default();
    let mut current_running: Option<usize> = None;
    let mut in_failure_block = false;
    for line in lines {
        let trimmed = line.trim_end();
        if let Some(n) = parse_running_line(trimmed) {
            current_running = Some(n);
            in_failure_block = false;
            continue;
        }
        if let Some(suite) = parse_result_line(trimmed, current_running.take()) {
            summary.suites.push(suite);
            in_failure_block = false;
            continue;
        }
        if let Some(executed) = parse_test_status_line(trimmed) {
            summary.executed.push(executed);
            continue;
        }
        if trimmed == "failures:" {
            in_failure_block = true;
            summary.failure_lines.push(trimmed.to_string());
            continue;
        }
        if in_failure_block {
            // Stop capturing once the suite result line would fire (handled above).
            summary.failure_lines.push(trimmed.to_string());
        }
    }
    summary
}

pub(crate) fn evaluate_test_run(
    summary: &TestHarnessSummary,
    filter: Option<&str>,
    compiled_units: usize,
    cargo_success: bool,
    command: &str,
    elapsed: Duration,
    compiler_digest_lines: &[String],
) -> TestVerdict {
    let total_running: usize = summary.suites.iter().map(|s| s.running).sum();
    let total_passed: usize = summary.suites.iter().map(|s| s.passed).sum();
    let total_failed: usize = summary.suites.iter().map(|s| s.failed).sum();
    let total_ignored: usize = summary.suites.iter().map(|s| s.ignored).sum();
    let harness_ok = !summary.suites.is_empty() && summary.suites.iter().all(|s| s.ok);

    if let Some(reason) = zero_match_failure(filter, total_running, &summary.suites) {
        return failed_verdict(&reason, command, elapsed, summary, compiler_digest_lines);
    }
    if let Some(reason) = no_targets_failure(summary, compiled_units, cargo_success) {
        return failed_verdict(&reason, command, elapsed, summary, compiler_digest_lines);
    }
    if !cargo_success || !harness_ok || total_failed > 0 {
        let reason = format!(
            "failed: {total_passed} passed, {total_failed} failed, {total_ignored} ignored"
        );
        return failed_verdict(&reason, command, elapsed, summary, compiler_digest_lines);
    }
    let mut lines = vec![format!(
        "passed: {total_passed} passed, {total_failed} failed, {total_ignored} ignored; command: {command}; elapsed: {}",
        format_duration(elapsed)
    )];
    lines.extend(render_executed_names(summary));
    TestVerdict {
        success: true,
        exit_code: 0,
        digest: cap_digest_lines(lines).join("\n"),
    }
}

fn zero_match_failure(
    filter: Option<&str>,
    total_running: usize,
    suites: &[SuiteResult],
) -> Option<String> {
    let filter = filter?.trim();
    if filter.is_empty() {
        return None;
    }
    if suites.is_empty() || total_running == 0 {
        return Some(format!("failed: 0 tests matched filter '{filter}'"));
    }
    None
}

fn no_targets_failure(
    summary: &TestHarnessSummary,
    compiled_units: usize,
    cargo_success: bool,
) -> Option<String> {
    if !summary.suites.is_empty() {
        return None;
    }
    // No libtest suites observed: either no targets, compile-only failure, or empty run.
    if compiled_units == 0 || !cargo_success {
        return Some("failed: no test targets found".to_string());
    }
    Some("failed: 0 tests ran".to_string())
}

fn failed_verdict(
    headline: &str,
    command: &str,
    elapsed: Duration,
    summary: &TestHarnessSummary,
    compiler_digest_lines: &[String],
) -> TestVerdict {
    let mut lines = vec![format!(
        "{headline}; command: {command}; elapsed: {}",
        format_duration(elapsed)
    )];
    lines.extend(render_executed_names(summary));
    lines.extend(summary.failure_lines.iter().cloned());
    lines.extend(compiler_digest_lines.iter().cloned());
    TestVerdict {
        success: false,
        exit_code: 1,
        digest: cap_digest_lines(lines).join("\n"),
    }
}

fn render_executed_names(summary: &TestHarnessSummary) -> Vec<String> {
    if summary.executed.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!("ran {} test(s):", summary.executed.len())];
    for (idx, test) in summary.executed.iter().enumerate() {
        if idx >= MAX_NAMED_TESTS {
            let rest = summary.executed.len() - MAX_NAMED_TESTS;
            lines.push(format!("  ... {rest} more"));
            break;
        }
        lines.push(format!("  {} ({})", test.name, test.status.as_str()));
    }
    lines
}

fn parse_running_line(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("running ")?;
    let (num, tail) = rest.split_once(' ')?;
    if tail.starts_with("test") {
        return num.parse().ok();
    }
    None
}

fn parse_result_line(line: &str, running: Option<usize>) -> Option<SuiteResult> {
    let rest = line.strip_prefix("test result: ")?;
    let (status, stats) = rest.split_once(". ")?;
    let ok = status == "ok";
    let mut suite = SuiteResult {
        running: running.unwrap_or(0),
        ok,
        ..SuiteResult::default()
    };
    for part in stats.split(';') {
        let part = part.trim();
        if let Some(n) = strip_count(part, "passed") {
            suite.passed = n;
        } else if let Some(n) = strip_count(part, "failed") {
            suite.failed = n;
        } else if let Some(n) = strip_count(part, "ignored") {
            suite.ignored = n;
        } else if let Some(n) = strip_count(part, "filtered out") {
            suite.filtered_out = n;
        }
    }
    if running.is_none() {
        suite.running = suite.passed + suite.failed + suite.ignored;
    }
    Some(suite)
}

fn strip_count(part: &str, label: &str) -> Option<usize> {
    let (num, rest) = part.split_once(' ')?;
    if rest == label || rest.starts_with(label) {
        return num.parse().ok();
    }
    None
}

fn parse_test_status_line(line: &str) -> Option<ExecutedTest> {
    let rest = line.strip_prefix("test ")?;
    // "test foo::bar ... ok" / "FAILED" / "ignored"
    let (name, status_part) = rest.rsplit_once(" ... ")?;
    let status = match status_part.trim() {
        "ok" => TestStatus::Ok,
        "FAILED" => TestStatus::Failed,
        s if s.starts_with("ignored") => TestStatus::Ignored,
        _ => return None,
    };
    Some(ExecutedTest {
        name: name.trim().to_string(),
        status,
    })
}

impl TestStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "FAILED",
            Self::Ignored => "ignored",
        }
    }
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }
    format!("{:.1}s", millis as f64 / 1_000.0)
}

fn cap_digest_lines(mut lines: Vec<String>) -> Vec<String> {
    if lines.len() <= MAX_DIGEST_LINES {
        return lines;
    }
    let suppressed = lines.len() - MAX_DIGEST_LINES + 1;
    lines.truncate(MAX_DIGEST_LINES - 1);
    lines.push(format!("... {suppressed} more lines suppressed"));
    lines
}

#[cfg(test)]
#[path = "test_parse_tests.rs"]
mod tests;
