// Marker fixture and clock helpers for hold regression tests.
// Deps: parent rate-limit test imports, filesystem timestamps.

use super::*;

pub(super) fn read_fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path:?}: {err}"))
}

/// Backdate a marker's mtime so the bounded cooldown can be exercised without
/// sleeping. The transient class measures from the write time.
pub(super) fn age_marker(path: &std::path::Path, seconds: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap_or_else(|err| panic!("open {path:?}: {err}"));
    file.set_modified(when)
        .unwrap_or_else(|err| panic!("set mtime on {path:?}: {err}"));
}

/// Shift a fixture's stated time relative to now while preserving its provider
/// formatting. The message body remains the captured evidence under test.
pub(super) fn with_relative_recovery_time(content: &str, delta: Duration) -> String {
    let stated = content
        .lines()
        .find_map(|line| line.strip_prefix("recovery_at: "))
        .filter(|value| !value.is_empty())
        .expect("fixture must state a recovery time");
    let template_tokens: Vec<_> = stated.split_whitespace().collect();
    let at = Local::now().naive_local() + delta;
    let day_token = template_tokens[1];
    let day = if day_token.starts_with('0') {
        format!("{:02}", at.day())
    } else {
        at.day().to_string()
    };
    let suffix = if day_token.ends_with("st,")
        || day_token.ends_with("nd,")
        || day_token.ends_with("rd,")
        || day_token.ends_with("th,")
    {
        ordinal_suffix(at.day()).to_string()
    } else {
        String::new()
    };
    let hour = if template_tokens[3].starts_with('0') {
        format!("{:02}", at.hour12().1)
    } else {
        at.hour12().1.to_string()
    };
    let replacement = format!(
        "{} {}{}, {} {}:{:02} {}",
        at.format("%b"),
        day,
        suffix,
        at.year(),
        hour,
        at.minute(),
        at.format("%p")
    );
    content.replacen(
        &format!("recovery_at: {stated}"),
        &format!("recovery_at: {replacement}"),
        1,
    )
}

pub(super) fn ordinal_suffix(day: u32) -> &'static str {
    match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}
