// Claude quota reset clocks, rejected subscription events, and recovery suffixes.
// Exports: parse_claude_reset_clock, claude_rejected_limit, parse_recovery_time.
// Deps: chrono, serde_json, rate_limit recovery formatting.

use super::{format_recovery, parse_iso_recovery_time};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone, Utc};

pub(crate) fn parse_recovery_time(message: &str) -> Option<String> {
    // Claude's older refusal appends |<unix seconds>. The same suffix is
    // attached to a parsed rejected event when its structured resetsAt exists.
    if let Some((_, seconds)) = message.rsplit_once('|')
        && seconds.len() == 10
        && seconds.bytes().all(|byte| byte.is_ascii_digit())
        && let Ok(seconds) = seconds.parse::<i64>()
        && let Some(at) = DateTime::from_timestamp(seconds, 0)
    {
        return Some(format_recovery(at.with_timezone(&Local).naive_local()));
    }
    let prefix = "try again at ";
    if let Some(start) = message.find(prefix) {
        let start = start + prefix.len();
        let remainder = &message[start..];
        let end = remainder.find('.').unwrap_or(remainder.len());
        Some(remainder[..end].trim().to_string())
    } else {
        parse_iso_recovery_time(message)
    }
}

/// Claude's "resets 5pm (Asia/Shanghai)" gives a wall clock, not a duration.
/// Unknown zones fall back to the signature's bounded window.
pub(crate) fn parse_claude_reset_clock(lower: &str) -> Option<NaiveDateTime> {
    let rest = lower.split_once("resets ")?.1;
    let (clock, zone) = rest.split_once(" (")?;
    let offset = match zone.split_once(')')?.0 {
        "asia/shanghai" => FixedOffset::east_opt(8 * 3600)?,
        "utc" => FixedOffset::east_opt(0)?,
        _ => return None,
    };
    let clock = clock.trim();
    let (digits, meridiem) = clock.split_at(clock.len().checked_sub(2)?);
    let (hour, minute) = match digits.split_once(':') {
        Some((hour, minute)) => (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    if !(1..=12).contains(&hour) || minute > 59 {
        return None;
    }
    let hour = match meridiem {
        "am" => hour % 12,
        "pm" => hour % 12 + 12,
        _ => return None,
    };
    let now = Utc::now().with_timezone(&offset);
    let mut date = now.date_naive();
    if date.and_hms_opt(hour, minute, 0)? <= now.naive_local() {
        date = date.succ_opt()?;
    }
    let at = offset.from_local_datetime(&date.and_hms_opt(hour, minute, 0)?).single()?;
    Some(at.with_timezone(&Local).naive_local())
}

/// A parsed Claude subscription status is the only authority for this signal;
/// overageStatus can be "rejected" even while status is "allowed".
pub(crate) fn claude_rejected_limit(object: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let info = object.get("rate_limit_info")?;
    (info.get("status")?.as_str()? == "rejected").then(|| {
        match info.get("resetsAt").and_then(serde_json::Value::as_i64) {
            Some(seconds) => format!("You've hit your limit|{seconds}"),
            None => "You've hit your limit".to_string(),
        }
    })
}
