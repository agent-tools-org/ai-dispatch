// Deterministic Qwen reset-date parsing and UTC year inference.
// Deps: parent recovery parser and chrono timezone conversion.
use super::*;
use chrono::Timelike;

fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("timestamp")
        .with_timezone(&Utc)
}

#[test]
fn qwen_reset_in_january_rolls_forward_from_december() {
    let now = utc("2026-12-31T23:00:00Z");
    let reset = parse_reset_at_utc_from("reset at 01-06 10:12:00 utc", now).expect("reset");
    assert_eq!(reset, utc("2027-01-06T10:12:00Z"));
    assert!(reset.signed_duration_since(now) > Duration::hours(24));
}

#[test]
fn qwen_future_reset_keeps_the_current_utc_year() {
    let now = utc("2026-07-01T12:00:00Z");
    let reset = parse_reset_at_utc_from("reset at 07-06 10:12:00 utc", now).expect("reset");
    assert_eq!(reset, utc("2026-07-06T10:12:00Z"));
}

#[test]
fn qwen_reset_uses_utc_year_even_when_the_local_year_differs() {
    let now = DateTime::parse_from_rfc3339("2027-01-01T01:00:00+02:00")
        .expect("local timestamp")
        .with_timezone(&Utc);
    let reset = parse_reset_at_utc_from("reset at 12-31 23:30:00 utc", now).expect("reset");
    assert_eq!(reset, utc("2026-12-31T23:30:00Z"));
}

#[test]
fn qwen_reset_converts_the_target_instant_to_local_time() {
    let now = Utc::now();
    let reset = now + Duration::days(7);
    let message = format!("reset at {} utc", reset.format("%m-%d %H:%M:%S"));
    let parsed = parse_reset_at_utc(&message).expect("local reset");
    assert_eq!(
        parsed,
        reset
            .with_timezone(&Local)
            .naive_local()
            .with_nanosecond(0)
            .expect("seconds")
    );
}

#[test]
fn qwen_reset_rejects_invalid_or_unlabelled_dates() {
    let now = utc("2026-12-31T23:00:00Z");
    for message in [
        "reset at 13-06 10:12:00 utc",
        "reset at 01-06 10:12:00",
        "5-hour quota",
    ] {
        assert!(parse_reset_at_utc_from(message, now).is_none(), "{message}");
    }
}
