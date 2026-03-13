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

pub fn mark_rate_limited(agent: &AgentKind) {
    let path = marker_path(agent);
    let _ = fs::write(&path, "");
}

pub fn is_rate_limited(agent: &AgentKind) -> bool {
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

pub fn is_rate_limit_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("429")
        || lower.contains("quota exceeded")
        || lower.contains("too many requests")
        || lower.contains("usage limit")
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

        mark_rate_limited(&AgentKind::Codex);
        assert!(is_rate_limited(&AgentKind::Codex));

        let _ = std::fs::remove_file(marker_path(&AgentKind::Codex));
        assert!(!is_rate_limited(&AgentKind::Codex));
    }
}
