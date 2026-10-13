// Existing recovery parsing and expiration regressions.
// Deps: parent rate-limit facade and isolated aid home.

use super::*;
use crate::paths;

#[test]
fn test_is_rate_limited_returns_false_for_fresh_agents() {
    let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-fresh");
    let _guard = paths::AidHomeGuard::set(&temp_dir);
    std::fs::create_dir_all(paths::aid_dir()).ok();

    assert!(!is_rate_limited(&AgentKind::Codex, None));
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
        parse_recovery_time(
            "You have hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Mar 19th, 2026 2:27 PM."
        ),
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
    let path = marker_path(&AgentKind::Codex, None);
    let _ = std::fs::write(&path, content);

    assert!(!is_rate_limited(&AgentKind::Codex, None));

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_get_rate_limit_info() {
    let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-info");
    let _guard = paths::AidHomeGuard::set(&temp_dir);
    std::fs::create_dir_all(paths::aid_dir()).ok();

    // Test with recovery time
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        "You have hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Mar 19th, 2026 2:27 PM.",
    );
    let info = get_rate_limit_info(&AgentKind::Codex, None).unwrap();
    assert_eq!(info.recovery_at, Some("Mar 19th, 2026 2:27 PM".to_string()));
    assert!(
        info.message
            .unwrap()
            .contains("You have hit your usage limit")
    );

    // Test without recovery time
    mark_rate_limited(&AgentKind::Gemini, None, "rate limit exceeded");
    let info = get_rate_limit_info(&AgentKind::Gemini, None).unwrap();
    assert_eq!(info.recovery_at, None);
    assert_eq!(info.message, Some("rate limit exceeded".to_string()));

    mark_rate_limited(&AgentKind::Qwen, None, "rate limit exceeded");
    let info = get_rate_limit_info(&AgentKind::Qwen, None).unwrap();
    assert_eq!(info.recovery_at, None);
    assert_eq!(info.message, Some("rate limit exceeded".to_string()));

    // Test non-existent file
    assert!(get_rate_limit_info(&AgentKind::Cursor, None).is_none());

    let _ = std::fs::remove_file(marker_path(&AgentKind::Codex, None));
    let _ = std::fs::remove_file(marker_path(&AgentKind::Gemini, None));
    let _ = std::fs::remove_file(marker_path(&AgentKind::Qwen, None));
}
