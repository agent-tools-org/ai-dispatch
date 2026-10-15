// Tests for usage budget window parsing.
// Covers named budget windows, numeric suffix windows, and invalid values.
// Deps: super::parse_window, chrono::Duration.

use super::parse_window;
use chrono::Duration;

#[test]
fn parse_window_accepts_named_budget_windows() {
    assert_eq!(parse_window("daily"), Some(Duration::days(1)));
    assert_eq!(parse_window("day"), Some(Duration::days(1)));
    assert_eq!(parse_window("DAILY"), Some(Duration::days(1)));
    assert_eq!(parse_window("weekly"), Some(Duration::days(7)));
    assert_eq!(parse_window("week"), Some(Duration::days(7)));
    assert_eq!(parse_window("monthly"), Some(Duration::days(30)));
    assert_eq!(parse_window("month"), Some(Duration::days(30)));
}

#[test]
fn parse_window_accepts_numeric_budget_windows() {
    assert_eq!(parse_window("24h"), Some(Duration::hours(24)));
    assert_eq!(parse_window("7d"), Some(Duration::days(7)));
    assert_eq!(parse_window("30m"), Some(Duration::minutes(30)));
}

#[test]
fn parse_window_rejects_unknown_budget_windows() {
    assert_eq!(parse_window("daly"), None);
    assert_eq!(parse_window("garbage"), None);
    assert_eq!(parse_window(""), None);
}
