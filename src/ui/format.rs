// Shared display formatting for byte and character limits, tokens, and elapsed milliseconds.
// Exports plain formatting functions used by command and terminal interfaces.
use chrono::{DateTime, Local};

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

pub(crate) fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub(crate) fn format_duration_ms(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

pub(crate) fn truncate_chars(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    if width <= 3 {
        return value.chars().take(width).collect();
    }
    let prefix: String = value.chars().take(width - 3).collect();
    format!("{prefix}...")
}

pub(crate) fn format_duration_optional_ms(duration_ms: Option<i64>) -> String {
    duration_ms
        .map(format_duration_ms)
        .unwrap_or_else(|| "n/a".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_truncation_preserves_boundaries_and_small_limits() {
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "...");
        assert_eq!(truncate("abcd", 0), "...");
        assert_eq!(truncate("", 0), "");
        assert_eq!(truncate("ab你好cd", 7), "ab...");
        assert_eq!(truncate("ab😀cd", 7), "ab...");
        assert_eq!(truncate("a\nbcd", 4), "a...");
    }

    #[test]
    fn character_truncation_preserves_unicode_and_small_limits() {
        assert_eq!(truncate_chars("你好😀", 3), "你好😀");
        assert_eq!(truncate_chars("你好😀abc", 4), "你...");
        assert_eq!(truncate_chars("你好😀abc", 3), "你好😀");
        assert_eq!(truncate_chars("你好😀abc", 1), "你");
        assert_eq!(truncate_chars("你好😀abc", 0), "");
        assert_eq!(truncate_chars("", 0), "");
    }

    #[test]
    fn tokens_preserve_thresholds_and_signed_values() {
        for (value, expected) in [
            (-1, "-1"),
            (0, "0"),
            (999, "999"),
            (1_000, "1.0k"),
            (3_400, "3.4k"),
            (999_999, "1000.0k"),
            (1_200_000, "1.2M"),
        ] {
            assert_eq!(format_tokens(value), expected);
        }
    }

    #[test]
    fn milliseconds_preserve_sign_and_minute_padding() {
        for (value, expected) in [
            (-1_000, "-1s"),
            (-1, "0s"),
            (0, "0s"),
            (999, "0s"),
            (59_999, "59s"),
            (60_000, "1m 00s"),
            (63_000, "1m 03s"),
            (3_600_000, "60m 00s"),
        ] {
            assert_eq!(format_duration_ms(value), expected);
            assert_eq!(format_duration_optional_ms(Some(value)), expected);
        }
        assert_eq!(format_duration_optional_ms(None), "n/a");
    }
}

pub(crate) fn title_case(name: &str) -> String {
    name.split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn format_relative_time(value: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value) else {
        return value.to_string();
    };
    let seconds = Local::now()
        .signed_duration_since(parsed.with_timezone(&Local))
        .num_seconds();
    if seconds <= 0 {
        "just now".to_string()
    } else if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

pub(crate) fn elapsed_since(start: chrono::DateTime<chrono::Local>) -> String {
    let elapsed = chrono::Local::now() - start;
    let secs = elapsed.num_seconds();
    if secs < 0 {
        "0s".to_string()
    } else if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}
