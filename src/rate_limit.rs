// Rate-limit detection: marks agents as rate-limited when quota errors occur.
// Provides marker file tracking with a 5-minute cooldown for automatic budget mode.

use crate::paths::aid_dir;
use crate::types::AgentKind;
use chrono::{DateTime, Local, NaiveDateTime};
use std::fs;
use std::path::PathBuf;

const RATE_LIMIT_WINDOW_SECS: u64 = 300;

#[cfg(test)]
fn assert_marker_path_isolated() {
    crate::paths::assert_aid_home_isolated("rate_limit::marker_path");
}

fn marker_path(agent: &AgentKind) -> PathBuf {
    #[cfg(test)]
    assert_marker_path_isolated();
    aid_dir().join(format!("rate-limit-{}", agent.as_str()))
}

/// Marker for one model group of an agent whose plan meters families
/// separately. agy's gemini allowance can be exhausted while its claude
/// allowance still serves; a per-agent marker would strand the working one.
fn group_marker_path(agent: &AgentKind, group: &str) -> PathBuf {
    #[cfg(test)]
    assert_marker_path_isolated();
    aid_dir().join(format!("rate-limit-{}--{}", agent.as_str(), group))
}

pub fn mark_group_rate_limited(agent: &AgentKind, group: &str, message: &str) {
    write_marker(&group_marker_path(agent, group), message);
}

pub fn is_group_rate_limited(agent: &AgentKind, group: &str) -> bool {
    marker_is_active(&group_marker_path(agent, group))
}

pub fn clear_group_rate_limit(agent: &AgentKind, group: &str) -> bool {
    fs::remove_file(group_marker_path(agent, group)).is_ok()
}

pub fn mark_rate_limited(agent: &AgentKind, message: &str) {
    write_marker(&marker_path(agent), message);
}

fn write_marker(path: &std::path::Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let recovery_at = parse_recovery_time(message).or_else(|| resolved_recovery(message));
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
    let _ = fs::write(path, content);
}

/// Shared liveness check for a marker file: honour a parsed recovery time when
/// present, otherwise fall back to the mtime cooldown window.
fn marker_is_active(path: &std::path::Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let recovery = content
        .lines()
        .find_map(|line| line.strip_prefix("recovery_at: "))
        .filter(|value| !value.is_empty())
        .and_then(parse_recovery_datetime);
    if let Some(recovery_at) = recovery {
        return recovery_at > Local::now().naive_local();
    }
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed.as_secs() < RATE_LIMIT_WINDOW_SECS)
}

/// Clear a marker only when it predates `task_start`.
///
/// A successful task is not evidence that a provider has quota. When the same
/// run just observed a refusal and recorded it, clearing on "success" erases the
/// outage microseconds after it was captured and hands routing back a provider
/// that is out — which is how a marker written by `record_quota_exhaustion`
/// survived the watcher and then died in `handle_done_postprocess`.
///
/// Returns true when a marker was actually removed.
pub fn clear_rate_limit_if_stale(agent: &AgentKind, task_start: DateTime<Local>) -> bool {
    let path = marker_path(agent);
    let written_after_start = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .map(|modified| DateTime::<Local>::from(modified) >= task_start)
        .unwrap_or(false);
    if written_after_start {
        return false;
    }
    clear_rate_limit(agent)
}

pub fn clear_group_rate_limit_if_stale(
    agent: &AgentKind,
    group: &str,
    task_start: DateTime<Local>,
) -> bool {
    let path = group_marker_path(agent, group);
    let written_after_start = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .map(|modified| DateTime::<Local>::from(modified) >= task_start)
        .unwrap_or(false);
    if written_after_start {
        return false;
    }
    clear_group_rate_limit(agent, group)
}

pub fn clear_rate_limit_for_model_if_stale(
    agent: &AgentKind,
    model: Option<&str>,
    task_start: DateTime<Local>,
) -> bool {
    let mut cleared = clear_rate_limit_if_stale(agent, task_start);
    if let Some(group) = crate::agent::model_group::model_group(*agent, model) {
        if clear_group_rate_limit_if_stale(agent, group, task_start) {
            cleared = true;
        }
    }
    cleared
}

pub fn clear_rate_limit_for_model(agent: &AgentKind, model: Option<&str>) -> bool {
    let mut cleared = clear_rate_limit(agent);
    if let Some(group) = crate::agent::model_group::model_group(*agent, model) {
        if clear_group_rate_limit(agent, group) {
            cleared = true;
        }
    }
    cleared
}

pub fn clear_rate_limit(agent: &AgentKind) -> bool {
    fs::remove_file(marker_path(agent)).is_ok()
}

pub fn clear_all_rate_limits_for_agent(agent: &AgentKind) -> bool {
    let mut cleared = clear_rate_limit(agent);
    for (group, _) in crate::agent::model_group::groups_for_agent(*agent) {
        if clear_group_rate_limit(agent, group) {
            cleared = true;
        }
    }
    cleared
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

/// Absolute recovery time for messages that state their reset relatively
/// ("resets in 1 day", "5-hour quota") or not at all. Falling back to the
/// 5-minute default sent work straight back to a provider that was still
/// refusing it — droid's weekly cap read as recovered after five minutes.
fn resolved_recovery(message: &str) -> Option<String> {
    let format_at =
        |at: chrono::NaiveDateTime| at.format("%b %d, %Y %I:%M %p").to_string();
    if let Some(at) = crate::rate_limit_signatures::parse_relative_recovery(message) {
        return Some(format_at(at));
    }
    let (_, fallback_minutes) = crate::rate_limit_signatures::match_quota_signature(message)?;
    Some(format_at(
        Local::now().naive_local() + chrono::Duration::minutes(fallback_minutes),
    ))
}

/// The agent a quota message names, when the provider's wording identifies it.
/// Lets a caller mark the right agent even when the failure surfaced somewhere
/// that does not know which CLI produced it.
pub fn quota_signature_agent(message: &str) -> Option<AgentKind> {
    crate::rate_limit_signatures::match_quota_signature(message).map(|(agent, _)| agent)
}

/// Where a quota signal was observed. Generic tokens (429, "rate limit") are
/// evidence on channels the agent does not author; they must never match prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaEvidence {
    /// Structured CLI error events, stderr, HTTP status lines.
    NonAgentChannel,
    /// Assistant-authored text — per-agent templates only.
    AgentProse,
}

pub fn is_rate_limit_error(message: &str) -> bool {
    is_rate_limit_error_with_evidence(message, QuotaEvidence::AgentProse)
}

pub fn is_rate_limit_error_with_evidence(message: &str, evidence: QuotaEvidence) -> bool {
    if crate::rate_limit_signatures::match_quota_signature(message).is_some() {
        return true;
    }
    evidence == QuotaEvidence::NonAgentChannel && generic_quota_signal(message)
}

pub fn is_rate_limit_error_for_agent(message: &str, agent: &AgentKind) -> bool {
    is_rate_limit_error_for_agent_with_evidence(message, agent, QuotaEvidence::NonAgentChannel)
}

pub fn is_rate_limit_error_for_agent_with_evidence(
    message: &str,
    agent: &AgentKind,
    evidence: QuotaEvidence,
) -> bool {
    if crate::rate_limit_signatures::match_quota_signature_for_agent(message, *agent).is_some() {
        return true;
    }
    evidence == QuotaEvidence::NonAgentChannel && generic_quota_signal(message)
}

fn generic_quota_signal(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || contains_status_code(&lower, "429")
        || contains_status_code(&lower, "402")
        || lower.contains("too many requests")
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

pub fn extract_rate_limit_message(raw: &str) -> Option<String> {
    extract_rate_limit_with_evidence(raw, QuotaEvidence::NonAgentChannel, None)
}

/// Extract a quota refusal from a streaming event's `detail` field.
/// JSON error envelopes are a genuine non-agent channel; plain text is
/// assistant-authored and only matches per-agent templates.
pub fn extract_rate_limit_from_stream_detail(raw: &str, agent: &AgentKind) -> Option<String> {
    extract_rate_limit_with_evidence(raw, QuotaEvidence::AgentProse, Some(agent))
}

fn extract_rate_limit_with_evidence(
    raw: &str,
    prose_evidence: QuotaEvidence,
    agent: Option<&AgentKind>,
) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') && trimmed.contains("\"type\"") {
        return extract_from_json_error(trimmed);
    }
    let matches = match agent {
        Some(agent) => {
            is_rate_limit_error_for_agent_with_evidence(trimmed, agent, prose_evidence)
        }
        None => is_rate_limit_error_with_evidence(trimmed, prose_evidence),
    };
    if matches && trimmed.len() < 500 {
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
    if let Some(message) = json_error_message(&value)
        && is_rate_limit_error_with_evidence(message, QuotaEvidence::NonAgentChannel)
    {
        return Some(message.to_string());
    }
    if is_rate_limit_error_with_evidence(json_str, QuotaEvidence::NonAgentChannel) {
        return Some(json_str.chars().take(240).collect());
    }
    None
}

fn json_error_message(value: &serde_json::Value) -> Option<&str> {
    value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.pointer("/error/message").and_then(serde_json::Value::as_str))
        .or_else(|| value.pointer("/error/data/message").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|message| !message.is_empty())
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

pub fn recovery_datetime(agent: &AgentKind) -> Option<NaiveDateTime> {
    let recovery_at = get_rate_limit_info(agent)?.recovery_at?;
    parse_recovery_datetime(&recovery_at)
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

    /// The exact string codex produced on 2026-08-05. If this fails to parse,
    /// `is_rate_limited` falls back to a 300-second mtime window and a six-day
    /// outage reads as available again after five minutes.
    #[test]
    fn codex_recovery_timestamp_parses() {
        let message = "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
                       to purchase more credits or try again at Aug 11th, 2026 2:23 PM.";
        let extracted = parse_recovery_time(message).expect("recovery phrase must be extracted");
        assert_eq!(extracted, "Aug 11th, 2026 2:23 PM");
        let parsed = parse_recovery_datetime(&extracted).expect("recovery timestamp must parse");
        assert!(parsed > Local::now().naive_local(), "parsed {parsed} must be in the future");
    }

    #[test]
    fn test_is_rate_limit_error() {
        assert!(is_rate_limit_error(
            "You have hit your usage limit. try again at Mar 19th, 2026 2:27 PM."
        ));
        assert!(is_rate_limit_error(
            "Quota exhausted: Your token-plan 5-hour quota has been exhausted."
        ));
        assert!(is_rate_limit_error("APIError: Insufficient balance. Manage your billing here"));
        assert!(is_rate_limit_error("402 payment required: reload your tokens"));
        assert!(is_rate_limit_error(
            "IneligibleTierError: This client is no longer supported for Gemini Code Assist for individuals; migrate to Antigravity"
        ));
        assert!(!is_rate_limit_error("network timeout"));
        assert!(!is_rate_limit_error("connection refused"));
        assert!(!is_rate_limit_error("payment required"));
        assert!(!is_rate_limit_error(
            "503 No accounts with a plan supporting gpt-4.1-nano"
        ));
        assert!(!is_rate_limit_error(
            "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)"
        ));
        assert!(!is_rate_limit_error("invoice 1402 created"));
    }

    #[test]
    fn prose_mentions_rate_limit_is_not_quota_failure() {
        assert!(!is_rate_limit_error(
            "rate_limit_kind now returns AgentKind::Custom for custom agents"
        ));
        assert!(!is_rate_limit_error(
            "The RPC provider throttles us; we saw a 429 and burned Alchemy credits"
        ));
        assert!(!is_rate_limit_error(
            "We must respect the rate limit on the Base sequencer feed"
        ));
        assert!(!is_rate_limit_error("The parser handles nested arrays correctly"));
    }

    #[test]
    fn generic_quota_signals_apply_on_non_agent_channels() {
        assert!(is_rate_limit_error_with_evidence(
            "rate limit exceeded",
            QuotaEvidence::NonAgentChannel
        ));
        assert!(is_rate_limit_error_with_evidence(
            "HTTP 429 Too Many Requests",
            QuotaEvidence::NonAgentChannel
        ));
        assert!(is_rate_limit_error_for_agent_with_evidence(
            "rate limit exceeded",
            &AgentKind::Claude,
            QuotaEvidence::NonAgentChannel
        ));
        assert!(is_rate_limit_error_for_agent_with_evidence(
            "429 Too Many Requests",
            &AgentKind::Grok,
            QuotaEvidence::NonAgentChannel
        ));
        assert!(!is_rate_limit_error_with_evidence(
            "We must respect the rate limit on the Base sequencer feed",
            QuotaEvidence::AgentProse
        ));
    }

    #[test]
    fn extract_rate_limit_message_from_nested_429_json() {
        assert_eq!(
            extract_rate_limit_message(
                r#"{"type":"error","error":{"message":"429 rate limit exceeded"}}"#
            ),
            Some("429 rate limit exceeded".to_string())
        );
    }

    #[test]
    fn test_rate_limit_window_matches_five_minutes() {
        assert_eq!(RATE_LIMIT_WINDOW_SECS, 300);
    }

    #[test]
    fn test_extract_rate_limit_message_plain_text() {
        assert_eq!(
            extract_rate_limit_message("You have hit your usage limit."),
            Some("You have hit your usage limit.".to_string())
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
            extract_rate_limit_message(
                r#"{"type":"error","message":"You have hit your usage limit."}"#
            ),
            Some("You have hit your usage limit.".to_string())
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
    fn stream_detail_ignores_agent_grep_about_rate_limit_code() {
        let grep_line = "completed: grep clear_rate_limit_if_stale|marker_path";
        assert!(
            extract_rate_limit_message(grep_line).is_some(),
            "generic matcher still fires on stderr/log channels"
        );
        assert_eq!(
            extract_rate_limit_from_stream_detail(grep_line, &AgentKind::Cursor),
            None,
        );
    }

    #[test]
    fn stream_detail_extracts_codex_usage_limit_json() {
        let message = "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
                         to purchase more credits or try again at Aug 11th, 2026 2:23 PM.";
        assert_eq!(
            extract_rate_limit_from_stream_detail(
                &format!(r#"{{"type":"error","message":"{message}"}}"#),
                &AgentKind::Codex,
            ),
            Some(message.to_string()),
        );
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

#[cfg(test)]
mod stale_clear_tests {
    use super::*;
    use crate::types::AgentKind;

    /// A marker this run wrote must outlive this run's success. Clearing it is
    /// how an outage recorded by `record_quota_exhaustion` was handed straight
    /// back to routing by `handle_done_postprocess`.
    #[test]
    fn a_marker_written_during_the_run_is_not_cleared_by_success() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        clear_rate_limit(&AgentKind::Qwen);

        let task_start = Local::now() - chrono::Duration::minutes(5);
        mark_rate_limited(&AgentKind::Qwen, "Your token-plan 5-hour quota has been exhausted.");

        assert!(!clear_rate_limit_if_stale(&AgentKind::Qwen, task_start));
        assert!(is_rate_limited(&AgentKind::Qwen));
    }

    /// A marker left by an earlier run is stale and a fresh success clears it,
    /// exactly as before this change.
    #[test]
    fn a_marker_from_an_earlier_run_is_still_cleared() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        clear_rate_limit(&AgentKind::Qwen);

        mark_rate_limited(&AgentKind::Qwen, "Your token-plan 5-hour quota has been exhausted.");
        let task_start = Local::now() + chrono::Duration::minutes(5);

        assert!(clear_rate_limit_if_stale(&AgentKind::Qwen, task_start));
        assert!(!is_rate_limited(&AgentKind::Qwen));
    }

    #[test]
    fn clear_group_rate_limit_if_stale_clears_only_matching_group() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let agent = AgentKind::Antigravity;
        clear_all_rate_limits_for_agent(&agent);

        mark_rate_limited(&agent, "Agent rate limit");
        mark_group_rate_limited(&agent, "gemini", "Gemini quota exhausted");
        mark_group_rate_limited(&agent, "claude", "Claude quota exhausted");

        let task_start = Local::now() + chrono::Duration::minutes(5);

        let cleared = clear_rate_limit_for_model_if_stale(&agent, Some("gemini-3.6-flash-high"), task_start);
        assert!(cleared, "gemini group marker should be cleared on success");

        assert!(!is_rate_limited(&agent), "agent-level marker must be cleared on model success");
        assert!(!is_group_rate_limited(&agent, "gemini"), "gemini group must no longer be limited");
        assert!(is_group_rate_limited(&agent, "claude"), "claude group must remain limited");
    }

    #[test]
    fn clear_rate_limit_does_not_clear_group_markers() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let agent = AgentKind::Antigravity;
        clear_all_rate_limits_for_agent(&agent);

        mark_rate_limited(&agent, "Agent level limit");
        mark_group_rate_limited(&agent, "gemini", "Gemini quota exhausted");

        assert!(clear_rate_limit(&agent));
        assert!(!is_rate_limited(&agent), "agent-level marker must be removed");
        assert!(is_group_rate_limited(&agent, "gemini"), "group marker must NOT be removed by clear_rate_limit");
    }

    #[test]
    fn clear_all_rate_limits_clears_agent_and_all_groups() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let agent = AgentKind::Antigravity;
        clear_all_rate_limits_for_agent(&agent);

        mark_rate_limited(&agent, "Agent limit");
        mark_group_rate_limited(&agent, "gemini", "Gemini limit");
        mark_group_rate_limited(&agent, "claude", "Claude limit");

        assert!(clear_all_rate_limits_for_agent(&agent));
        assert!(!is_rate_limited(&agent));
        assert!(!is_group_rate_limited(&agent, "gemini"));
        assert!(!is_group_rate_limited(&agent, "claude"));
    }
}

#[cfg(test)]
mod home_guard_tests {
    use super::*;
    use crate::paths::{self, AidHomeGuard};

    #[test]
    fn marker_path_writes_under_isolated_home() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(paths::aid_dir()).unwrap();

        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        let marker = paths::aid_dir().join("rate-limit-codex");
        assert!(marker.exists());
        assert!(marker.starts_with(temp.path()));
    }

    #[test]
    fn marker_path_refuses_real_home_without_guard() {
        let resolved = paths::aid_dir();
        let home = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let real = home.join(".aid");
        if resolved != real {
            return;
        }
        let err = std::panic::catch_unwind(|| {
            let _ = marker_path(&AgentKind::Codex);
        });
        assert!(
            err.is_err(),
            "marker_path must refuse real ~/.aid without AidHomeGuard"
        );
    }
}
