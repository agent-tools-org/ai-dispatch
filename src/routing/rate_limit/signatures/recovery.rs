// Recovery clocks admitted by each provider's reset format.
// Keeps duration and timestamp parsing adjacent to a provider-owned reset phrase.

use super::cursor;
use crate::types::AgentKind;
use chrono::{Datelike, Duration, Local, NaiveDateTime};

pub(crate) fn recovery_for_agent(message: &str, agent: AgentKind) -> Option<NaiveDateTime> {
    use crate::routing::rate_limit::{parse_recovery_datetime, parse_recovery_time};
    let lower = message.to_lowercase();
    if let Some(rest) = lower.trim_start().strip_prefix("try again at ") {
        return super::super::detection::parse_iso_recovery_time(rest)
            .or_else(|| parse_recovery_time(message))
            .as_deref().and_then(parse_recovery_datetime);
    }
    if super::match_quota_signature_for_agent(message, agent).is_none()
        && !(agent == AgentKind::Codex && lower.starts_with("codex rate limit exceeded"))
    {
        return None;
    }
    match agent {
        AgentKind::Claude => parse_recovery_time(message)
            .as_deref()
            .and_then(parse_recovery_datetime)
            .or_else(|| crate::routing::rate_limit::parse_claude_reset_clock(&lower)),
        AgentKind::Codex => {
            let rest = lower.split_once("try again at ")?.1;
            super::super::detection::parse_iso_recovery_time(rest)
                .or_else(|| parse_recovery_time(message))
                .as_deref()
                .and_then(parse_recovery_datetime)
        }
        AgentKind::Cursor => cursor::cycle_end(&lower),
        AgentKind::Droid | AgentKind::Antigravity => {
            let rest = lower.split_once("resets in ")?.1;
            let reset = format!("resets in {rest}");
            parse_relative_recovery(&reset)
        }
        AgentKind::Qwen => parse_reset_at_utc(&lower),
        _ => None,
    }
}

/// Parse relative reset phrasings that `parse_recovery_time`'s "try again at
/// <date>" format cannot reach: "resets in 1 day", "resets in 3 hours",
/// Returns an absolute local time; a window length alone is not a reset.
pub(crate) fn parse_relative_recovery(message: &str) -> Option<NaiveDateTime> {
    let lower = message.to_lowercase();
    let now = Local::now().naive_local();
    if let Some(at) = crate::routing::rate_limit::parse_claude_reset_clock(&lower) {
        return Some(at);
    }
    if let Some(at) = cursor::cycle_end(&lower) {
        return Some(at);
    }
    if let Some(duration) = parse_compact_duration(&lower) {
        return Some(now + duration);
    }
    if let Some(duration) = parse_resets_in(&lower) {
        return Some(now + duration);
    }
    if let Some(at) = parse_reset_at_utc(&lower) {
        return Some(at);
    }
    None
}

/// "the quota will reset at 08-05 15:12:00 utc" — an absolute instant with no
/// year, which is how qwen's token plan reports its window. Assumes the current
/// year and rolls to next year if that would land in the past.
fn parse_reset_at_utc(lower: &str) -> Option<NaiveDateTime> {
    let idx = lower.find("reset at ")?;
    let rest = lower[idx + "reset at ".len()..].trim();
    let stamp: String = rest.chars().take(14).collect();
    let year = Local::now().naive_local().year();
    let candidate =
        NaiveDateTime::parse_from_str(&format!("{year}-{stamp}"), "%Y-%m-%d %H:%M:%S").ok()?;
    let utc_offset = Local::now().offset().local_minus_utc();
    Some(candidate + Duration::seconds(i64::from(utc_offset)))
}

/// "resets in 59m21s" / "1h30m" — the compact form agy uses. The spaced parser
/// cannot read it, and falling through to a default cooldown turned a 59-minute
/// outage into a multi-hour one.
fn parse_compact_duration(lower: &str) -> Option<Duration> {
    let idx = lower
        .find("resets in ")
        .or_else(|| lower.find("try again in "))?;
    let rest = lower[idx..].split_once(" in ")?.1.trim();
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, 'd' | 'h' | 'm' | 's'))
        .collect();
    if token.is_empty() || !token.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut total = Duration::zero();
    let mut number = String::new();
    for ch in token.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let amount: i64 = number.parse().ok()?;
        number.clear();
        total += match ch {
            'd' => Duration::days(amount),
            'h' => Duration::hours(amount),
            'm' => Duration::minutes(amount),
            's' => Duration::seconds(amount),
            _ => return None,
        };
    }
    (total > Duration::zero()).then_some(total)
}

/// "resets in 1 day" / "resets in 45 minutes" / "try again in 2 hours"
fn parse_resets_in(lower: &str) -> Option<Duration> {
    let idx = lower
        .find("resets in ")
        .or_else(|| lower.find("try again in "))?;
    let rest = &lower[idx..];
    let rest = rest.split_once(" in ")?.1;
    let mut parts = rest.split_whitespace();
    let amount: i64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?.trim_end_matches([',', '.', ')']);
    unit_to_duration(unit, amount)
}

fn unit_to_duration(unit: &str, amount: i64) -> Option<Duration> {
    match unit {
        u if u.starts_with("minute") || u == "min" || u == "mins" => {
            Some(Duration::minutes(amount))
        }
        u if u.starts_with("hour") || u == "hr" || u == "hrs" => Some(Duration::hours(amount)),
        u if u.starts_with("day") => Some(Duration::days(amount)),
        u if u.starts_with("week") => Some(Duration::weeks(amount)),
        _ => None,
    }
}
