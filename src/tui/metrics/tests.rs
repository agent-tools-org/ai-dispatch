// Unit tests for `ps` output parsing in TUI process metrics.
// Covers valid and invalid command output without spawning subprocesses.

use super::{ProcessMetrics, parse_ps_output};

#[test]
fn parses_cpu_and_rss_from_ps_output() {
    assert_eq!(
        parse_ps_output("12.3 46080\n"),
        Some(ProcessMetrics {
            cpu_percent: 12.3,
            memory_mb: 45.0,
        })
    );
}

#[test]
fn rejects_incomplete_ps_output() {
    assert_eq!(parse_ps_output("12.3\n"), None);
}
