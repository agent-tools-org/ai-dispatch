// Existing rate-limit refusal detection and date parsing extracted from the public facade.
// Deps: parent marker, route availability and signature helpers.

use super::*;

/// Where a quota signal was observed. Generic tokens (429, "rate limit") are
/// evidence on channels the agent does not author; they must never match prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaEvidence {
    /// Structured CLI error events, stderr, HTTP status lines.
    NonAgentChannel,
    /// Assistant-authored text — per-agent templates only.
    #[cfg(test)]
    AgentProse,
}

#[cfg(test)]
pub fn is_rate_limit_error(message: &str) -> bool {
    is_rate_limit_error_with_evidence(message, QuotaEvidence::AgentProse)
}

#[cfg(test)]
pub fn is_rate_limit_error_with_evidence(message: &str, evidence: QuotaEvidence) -> bool {
    if crate::routing::rate_limit::signatures::match_quota_signature(message).is_some() {
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
    if crate::routing::rate_limit::signatures::match_quota_signature_for_agent(message, *agent).is_some() {
        return true;
    }
    evidence == QuotaEvidence::NonAgentChannel && generic_quota_signal(message)
}

/// A status token that means "out of quota" whoever the provider is. Admissible
/// only inside an envelope the CLI opened — see `quota_channel::Attributable`.
///
/// `rate_limit` is deliberately absent. No provider writes its refusal in
/// snake_case; that spelling only ever appears in source, in a grep pattern or
/// in a report about this crate, so it could match nothing real and forge
/// plenty.
fn generic_quota_signal(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
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

/// The provider's refusal in `raw`, or `None` — the one way captured bytes
/// become a rate-limit marker.
///
/// `raw` is first split by `quota_channel` into what the CLI said and what the
/// model said, and only the CLI's part is matched. Callers name the channel they
/// read from; nothing else about the bytes is consulted, and in particular no
/// caller may hand this an adapter's rendered event detail, an assistant
/// message, or a tool result. Those are not channels — see `quota_channel`.
///
/// The evidence rule follows the split: CLI diagnostics may carry a bare status
/// token. Unsplit lines are handled separately as transient excerpts.
pub(crate) fn refusal_on_channel(
    raw: &str,
    agent: AgentKind,
    channel: crate::routing::quota::channel::Channel,
) -> Option<String> {
    let kept = crate::routing::quota::channel::provider_attributable(raw, agent, channel);
    if let Some(refusal) = crate::agent::stream_completion::quota_line(&kept.cli_diagnostic, agent) {
        return Some(refusal);
    }
    let generic = kept
        .cli_diagnostic
        .lines()
        .find(|line| generic_quota_signal(line))?;
    let refusal: String = generic.chars().take(240).collect();
    let refusal = refusal.trim();
    (!refusal.is_empty()).then(|| refusal.to_string())
}

pub(super) fn parse_iso_recovery_time(message: &str) -> Option<String> {
    let candidate: String = message.trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | ':' | '.' | '+'))
        .collect();
    let candidate = candidate.trim_end_matches(['.', ',', ';', ')', ']', '}']);
    let parsed = chrono::DateTime::parse_from_rfc3339(candidate).ok()?;
    Some(format_recovery(parsed.with_timezone(&Local).naive_local()))
}

pub(crate) fn parse_recovery_datetime(s: &str) -> Option<NaiveDateTime> {
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

#[cfg(test)]
pub(crate) fn test_future_recovery_time() -> String {
    let at = Local::now().naive_local() + chrono::Duration::days(1);
    format!(
        "{} {}{}, {} {}:{:02} {}",
        at.format("%b"),
        at.day(),
        test_ordinal_suffix(at.day()),
        at.year(),
        at.hour12().1,
        at.minute(),
        at.format("%p")
    )
}

#[cfg(test)]
fn test_ordinal_suffix(day: u32) -> &'static str {
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
