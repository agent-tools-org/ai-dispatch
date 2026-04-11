// Rate-limit detection: marks agents as rate-limited when quota errors occur.
// Provides marker file tracking with a 5-minute cooldown for automatic budget mode.

use crate::paths::aid_dir;
use crate::types::AgentKind;
use chrono::{Local, NaiveDateTime};
use std::fs;
use std::path::PathBuf;

const RATE_LIMIT_WINDOW_SECS: u64 = 300;

fn marker_path(agent: &AgentKind) -> PathBuf {
    aid_dir().join(format!("rate-limit-{}", agent.as_str()))
}

pub fn mark_rate_limited(agent: &AgentKind, message: &str) {
    let path = marker_path(agent);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let recovery_at = parse_recovery_time(message);
    let truncated_message = if message.len() > 200 {
        let mut end = 200;
        while !message.is_char_boundary(end) { end -= 1; }
        &message[..end]
    } else {
        message
    };
    let content = format!(
        "recovery_at: {}\nmessage: {}\n",
        recovery_at.unwrap_or_default(),
        truncated_message
    );
    let _ = fs::write(&path, content);
}

pub fn clear_rate_limit(agent: &AgentKind) -> bool {
    let path = marker_path(agent);
    fs::remove_file(&path).is_ok()
}

pub fn is_rate_limited(agent: &AgentKind) -> bool {
    if let Some(info) = get_rate_limit_info(agent) {
        let within_window = || {
            let path = marker_path(agent);
            let Ok(metadata) = fs::metadata(&path) else {
                return false;
            };
            let Ok(modified) = metadata.modified() else {
                return false;
            };
            let Ok(elapsed) = modified.elapsed() else {
                return false;
            };
            elapsed.as_secs() < RATE_LIMIT_WINDOW_SECS
        };
        // If we have recovery_at info, check if it's still in the future
        if let Some(recovery_str) = info.recovery_at {
            if let Some(recovery_at) = parse_recovery_datetime(&recovery_str) {
                recovery_at > Local::now().naive_local()
            } else {
                // Fall back to the mtime-based cooldown window
                within_window()
            }
        } else {
            // Fall back to the mtime-based cooldown window
            within_window()
        }
    } else {
        false
    }
}

pub fn rate_limited_agents() -> Vec<(AgentKind, String)> {
    AgentKind::ALL_BUILTIN.iter().copied()
    .filter_map(|agent| {
        let info = get_rate_limit_info(&agent)?;
        is_rate_limited(&agent).then(|| (agent, info.message.unwrap_or_default()))
    })
    .collect()
}

pub fn is_rate_limit_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || contains_status_code(&lower, "429")
        || contains_status_code(&lower, "402")
        || lower.contains("quota exceeded")
        || lower.contains("exhausted your capacity")
        || lower.contains("too many requests")
        || lower.contains("usage limit")
        || lower.contains("credits")
        || lower.contains("reload your tokens")
}

pub fn extract_rate_limit_message(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') && trimmed.contains("\"type\"") {
        return extract_from_json_error(trimmed);
    }
    if is_rate_limit_error(trimmed) && trimmed.len() < 500 {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn extract_from_json_error(json_str: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let is_error_event = value.get("error").is_some()
        || value.get("type").and_then(serde_json::Value::as_str) == Some("error");
    if !is_error_event {
        return None;
    }
    let message = value.get("message")?.as_str()?.trim();
    if message.is_empty() || !is_rate_limit_error(message) {
        return None;
    }
    Some(message.to_string())
}

/// Match an HTTP status code only as a standalone number, not inside larger numbers.
fn contains_status_code(s: &str, code: &str) -> bool {
    let bytes = s.as_bytes();
    let code = code.as_bytes();
    if bytes.len() < code.len() {
        return false;
    }
    for i in 0..=bytes.len().saturating_sub(code.len()) {
        if &bytes[i..i + code.len()] == code {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let after_ok = i + code.len() >= bytes.len() || !bytes[i + code.len()].is_ascii_digit();
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

fn parse_recovery_time(message: &str) -> Option<String> {
    let prefix = "try again at ";
    if let Some(start) = message.find(prefix) {
        let start = start + prefix.len();
        let remainder = &message[start..];
        let end = remainder.find('.').unwrap_or(remainder.len());
        Some(remainder[..end].trim().to_string())
    } else {
        None
    }
}

fn parse_recovery_datetime(s: &str) -> Option<NaiveDateTime> {
    let mut parts: Vec<String> = s.split(' ').map(|part| part.to_string()).collect();
    if parts.len() < 2 {
        return None;
    }

    let day_token = &parts[1];
    let day_without_comma = day_token.strip_suffix(',').unwrap_or(day_token);
    let day_without_suffix = day_without_comma
        .strip_suffix("st")
        .or_else(|| day_without_comma.strip_suffix("nd"))
        .or_else(|| day_without_comma.strip_suffix("rd"))
        .or_else(|| day_without_comma.strip_suffix("th"))
        .unwrap_or(day_without_comma);
    let day_number: u32 = day_without_suffix.parse().ok()?;
    let day_with_comma = if day_token.ends_with(',') { "," } else { "" };
    parts[1] = format!("{:02}{}", day_number, day_with_comma);

    let cleaned = parts.join(" ");
    NaiveDateTime::parse_from_str(&cleaned, "%b %d, %Y %I:%M %p").ok()
}

#[derive(Debug, PartialEq)]
pub struct RateLimitInfo {
    pub recovery_at: Option<String>,
    pub message: Option<String>,
}

pub fn get_rate_limit_info(agent: &AgentKind) -> Option<RateLimitInfo> {
    let path = marker_path(agent);
    let content = fs::read_to_string(&path).ok()?;
    let mut recovery_at = None;
    let mut message = None;
    for line in content.lines() {
        if let Some(recovery) = line.strip_prefix("recovery_at: ") {
            recovery_at = if recovery.is_empty() {
                None
            } else {
                Some(recovery.to_string())
            };
        } else if let Some(msg) = line.strip_prefix("message: ") {
            message = if msg.is_empty() {
                None
            } else {
                Some(msg.to_string())
            };
        }
    }
    Some(RateLimitInfo {
        recovery_at,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    #[test]
    fn test_is_rate_limit_error() {
        assert!(is_rate_limit_error("rate limit exceeded"));
        assert!(is_rate_limit_error("RATE LIMIT"));
        assert!(is_rate_limit_error("error: rate_limit hit"));
        assert!(is_rate_limit_error("HTTP 429"));
        assert!(is_rate_limit_error("HTTP 402 Payment Required"));
        assert!(is_rate_limit_error("quota exceeded"));
        assert!(is_rate_limit_error(
            "You have exhausted your capacity for today."
        ));
        assert!(is_rate_limit_error("too many requests"));
        assert!(is_rate_limit_error("usage limit reached"));
        assert!(is_rate_limit_error("status: 429"));
        assert!(is_rate_limit_error("error 429 too many"));
        assert!(is_rate_limit_error("credits exhausted"));
        assert!(is_rate_limit_error("please reload your tokens"));
        assert!(!is_rate_limit_error("network timeout"));
        assert!(!is_rate_limit_error("connection refused"));
        assert!(!is_rate_limit_error("payment required"));
        assert!(!is_rate_limit_error(
            "503 No accounts with a plan supporting gpt-4.1-nano"
        ));
        // Must not match 429 inside larger numbers (token counts, IDs)
        assert!(!is_rate_limit_error(
            "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)"
        ));
        assert!(!is_rate_limit_error("invoice 1402 created"));
    }

    #[test]
    fn test_rate_limit_window_matches_five_minutes() {
        assert_eq!(RATE_LIMIT_WINDOW_SECS, 300);
    }

    #[test]
    fn test_extract_rate_limit_message_plain_text() {
        assert_eq!(
            extract_rate_limit_message("rate limit exceeded"),
            Some("rate limit exceeded".to_string())
        );
    }

    #[test]
    fn test_extract_rate_limit_message_ignores_init_json() {
        assert_eq!(
            extract_rate_limit_message(r#"{"type":"system","subtype":"init","message":"rate limit enabled"}"#),
            None
        );
    }

    #[test]
    fn test_extract_rate_limit_message_from_error_json() {
        assert_eq!(
            extract_rate_limit_message(r#"{"type":"error","message":"429 Too Many Requests"}"#),
            Some("429 Too Many Requests".to_string())
        );
    }

    #[test]
    fn test_extract_rate_limit_message_from_402_error_json() {
        assert_eq!(
            extract_rate_limit_message(
                r#"{"type":"error","source":"agent_loop","message":"402 payment required: reload your tokens"}"#
            ),
            Some("402 payment required: reload your tokens".to_string())
        );
    }

    #[test]
    fn test_extract_rate_limit_message_ignores_noise() {
        assert_eq!(extract_rate_limit_message("YOLO mode is enabled"), None);
    }

    #[test]
    fn test_mark_and_check_rate_limited() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        assert!(is_rate_limited(&AgentKind::Codex));

        let _ = std::fs::remove_file(marker_path(&AgentKind::Codex));
        assert!(!is_rate_limited(&AgentKind::Codex));
    }

    #[test]
    fn test_is_rate_limited_returns_false_for_fresh_agents() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-fresh");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        assert!(!is_rate_limited(&AgentKind::Codex));
    }

    #[test]
    fn test_rate_limited_agents_returns_empty_initially() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-empty");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        assert!(rate_limited_agents().is_empty());
    }

    #[test]
    fn test_parse_recovery_time() {
        assert_eq!(
            parse_recovery_time("You have hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Mar 19th, 2026 2:27 PM."),
            Some("Mar 19th, 2026 2:27 PM".to_string())
        );
        assert_eq!(parse_recovery_time("no recovery time here"), None);
        assert_eq!(
            parse_recovery_time("try again at tomorrow morning."),
            Some("tomorrow morning".to_string())
        );
    }

    #[test]
    fn test_parse_recovery_datetime() {
        let parsed = parse_recovery_datetime("Mar 19th, 2026 2:27 PM").unwrap();
        let expected =
            NaiveDateTime::parse_from_str("Mar 19, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(parsed, expected);

        let first = parse_recovery_datetime("Mar 1st, 2026 2:27 PM").unwrap();
        let expected_first =
            NaiveDateTime::parse_from_str("Mar 01, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(first, expected_first);

        let second = parse_recovery_datetime("Mar 2nd, 2026 2:27 PM").unwrap();
        let expected_second =
            NaiveDateTime::parse_from_str("Mar 02, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(second, expected_second);

        let third = parse_recovery_datetime("Mar 3rd, 2026 2:27 PM").unwrap();
        let expected_third =
            NaiveDateTime::parse_from_str("Mar 03, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(third, expected_third);

        assert!(parse_recovery_datetime("not a date").is_none());
    }

    #[test]
    fn test_is_rate_limited_expired() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-expired");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        let past = Local::now().naive_local() - chrono::Duration::minutes(5);
        let recovery_at = past.format("%b %d, %Y %I:%M %p").to_string();
        let content = format!("recovery_at: {}\nmessage: test\n", recovery_at);
        let path = marker_path(&AgentKind::Codex);
        let _ = std::fs::write(&path, content);

        assert!(!is_rate_limited(&AgentKind::Codex));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_get_rate_limit_info() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-info");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        // Test with recovery time
        mark_rate_limited(&AgentKind::Codex, "You have hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Mar 19th, 2026 2:27 PM.");
        let info = get_rate_limit_info(&AgentKind::Codex).unwrap();
        assert_eq!(info.recovery_at, Some("Mar 19th, 2026 2:27 PM".to_string()));
        assert!(info
            .message
            .unwrap()
            .contains("You have hit your usage limit"));

        // Test without recovery time
        mark_rate_limited(&AgentKind::Gemini, "rate limit exceeded");
        let info = get_rate_limit_info(&AgentKind::Gemini).unwrap();
        assert_eq!(info.recovery_at, None);
        assert_eq!(info.message, Some("rate limit exceeded".to_string()));

        mark_rate_limited(&AgentKind::Qwen, "rate limit exceeded");
        let info = get_rate_limit_info(&AgentKind::Qwen).unwrap();
        assert_eq!(info.recovery_at, None);
        assert_eq!(info.message, Some("rate limit exceeded".to_string()));

        // Test non-existent file
        assert!(get_rate_limit_info(&AgentKind::Cursor).is_none());

        let _ = std::fs::remove_file(marker_path(&AgentKind::Codex));
        let _ = std::fs::remove_file(marker_path(&AgentKind::Gemini));
        let _ = std::fs::remove_file(marker_path(&AgentKind::Qwen));
    }
}
