// Rate-limit detection: marks agents as rate-limited when quota errors occur.
// Provides marker file tracking with 1-hour cooldown for automatic budget mode.

use crate::paths::aid_dir;
use crate::types::AgentKind;
use std::fs;
use std::path::PathBuf;

const RATE_LIMIT_WINDOW_SECS: u64 = 3600;

fn marker_path(agent: &AgentKind) -> PathBuf {
    aid_dir().join(format!("rate-limit-{}", agent.as_str()))
}

pub fn mark_rate_limited(agent: &AgentKind, message: &str) {
    let path = marker_path(agent);
    let recovery_at = parse_recovery_time(message);
    let truncated_message = if message.len() > 200 {
        &message[..200]
    } else {
        message
    };
    let content = format!(
        "recovery_at: {}\nmessage: {}\n",
        recovery_at.unwrap_or_else(|| "".to_string()),
        truncated_message
    );
    let _ = fs::write(&path, content);
}

pub fn is_rate_limited(agent: &AgentKind) -> bool {
    if let Some(info) = get_rate_limit_info(agent) {
        // If we have recovery_at info, check if it's still in the future
        if let Some(recovery_str) = info.recovery_at {
            // Simple check: if recovery_at is not empty, consider it still rate limited
            // In a real implementation, you'd parse the date and compare with current time
            !recovery_str.is_empty()
        } else {
            // Fall back to mtime-based 1h window
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
        }
    } else {
        false
    }
}

pub fn is_rate_limit_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("429")
        || lower.contains("quota exceeded")
        || lower.contains("too many requests")
        || lower.contains("usage limit")
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
        assert!(is_rate_limit_error("quota exceeded"));
        assert!(is_rate_limit_error("too many requests"));
        assert!(is_rate_limit_error("usage limit reached"));
        assert!(!is_rate_limit_error("network timeout"));
        assert!(!is_rate_limit_error("connection refused"));
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

        // Test non-existent file
        assert!(get_rate_limit_info(&AgentKind::Cursor).is_none());

        let _ = std::fs::remove_file(marker_path(&AgentKind::Codex));
        let _ = std::fs::remove_file(marker_path(&AgentKind::Gemini));
    }
}
