// Per-CLI quota-exhaustion signatures and relative recovery-time parsing.
// Exports: match_quota_signature, parse_relative_recovery.
// Deps: types::AgentKind, chrono.

use crate::types::AgentKind;
use chrono::{Datelike, Duration, Local, NaiveDateTime};

/// A provider's quota message, captured from a real run, plus how long that
/// quota class actually lasts. The generic phrase list in `rate_limit.rs` misses
/// these: qwen says "quota has been exhausted" where the generic matcher only
/// knows "quota exceeded", so an exhausted qwen kept looking healthy and every
/// run it refused was recorded as a success.
pub(crate) struct QuotaSignature {
    pub(crate) agent: AgentKind,
    /// Lowercase substring taken verbatim from captured CLI output.
    pub(crate) needle: &'static str,
    /// Fallback cooldown when the message carries no parseable reset time.
    /// A wrong-but-short guess is worse than none: it sends work back to a
    /// provider that is still exhausted.
    pub(crate) fallback_minutes: i64,
}

pub(crate) const QUOTA_SIGNATURES: &[QuotaSignature] = &[
    // qwen 0.21.5, ModelStudio token plan, captured 2026-08-05:
    // "Quota exhausted: Your token-plan 5-hour quota has been exhausted."
    QuotaSignature { agent: AgentKind::Qwen, needle: "quota has been exhausted", fallback_minutes: 300 },
    QuotaSignature { agent: AgentKind::Qwen, needle: "quota exhausted", fallback_minutes: 300 },
    // droid 0.183.0, captured 2026-08-05 as an HTTP 402 body:
    // "You've reached your weekly standard usage limit (resets in 1 day)."
    QuotaSignature { agent: AgentKind::Droid, needle: "weekly standard usage limit", fallback_minutes: 1440 },
    // codex-cli, captured previously:
    // "You have hit your usage limit ... try again at <date>."
    QuotaSignature { agent: AgentKind::Codex, needle: "hit your usage limit", fallback_minutes: 300 },
    // oz (Warp cloud agents), captured 2026-08-05 with exit code 1:
    // "Error: Quota limit reached."
    // No reset time is given at all, so the cooldown is a guess; an hour keeps
    // the agent out of rotation without writing it off for the day.
    QuotaSignature { agent: AgentKind::Oz, needle: "quota limit reached", fallback_minutes: 60 },
    // agy 1.1.10, captured 2026-08-05 against the gemini group while the claude
    // group was still serving:
    // "Individual quota reached. Please upgrade your subscription to increase
    //  your limits. Resets in 59m21s."
    // The earlier entry here used the bare needle "quota" with an invented
    // 12-hour cooldown; it matched unrelated output and would have stranded a
    // working claude allowance for twelve hours over a 59-minute gemini outage.
    QuotaSignature { agent: AgentKind::Antigravity, needle: "individual quota reached", fallback_minutes: 60 },
    // opencode Zen, captured 2026-08-05 from t-76181278 as an HTTP 401 body:
    // {"type":"error","error":{"name":"APIError","data":{"message":"Insufficient
    //  balance. Manage your billing here: ...","statusCode":401}}}
    //
    // Unlike every other entry here this is not a time-based quota and it does
    // not recover on its own — it ends when the account is topped up. The table
    // has no way to say that, so the cooldown is a day: long enough to stop aid
    // feeding work to an account that cannot pay for it, short enough that a
    // top-up is not ignored for a week. `aid config clear-limit opencode` is the
    // escape hatch after paying.
    //
    // Neither the generic phrase list nor a status-code check caught this: 401
    // is neither 429 nor 402, and no needle contained "insufficient balance", so
    // aid kept reporting opencode as OK and kept dispatching to it.
    QuotaSignature { agent: AgentKind::OpenCode, needle: "insufficient balance", fallback_minutes: 1440 },
    // OpenCode-compatible overlays share the same Zen billing refusal shape.
    QuotaSignature { agent: AgentKind::MiMoCode, needle: "insufficient balance", fallback_minutes: 1440 },
    QuotaSignature { agent: AgentKind::Kilo, needle: "insufficient balance", fallback_minutes: 1440 },
    // droid 0.183.0, captured as HTTP 402 body:
    // "402 payment required: reload your tokens"
    QuotaSignature { agent: AgentKind::Droid, needle: "reload your tokens", fallback_minutes: 1440 },
    // gemini Code Assist, captured 2026-08-05:
    // "IneligibleTierError: ... migrate to Antigravity"
    QuotaSignature { agent: AgentKind::Gemini, needle: "ineligibletier", fallback_minutes: 1440 },
    QuotaSignature { agent: AgentKind::Gemini, needle: "resource exhausted", fallback_minutes: 60 },
    QuotaSignature { agent: AgentKind::Gemini, needle: "resourceexhausted", fallback_minutes: 60 },
    QuotaSignature { agent: AgentKind::Antigravity, needle: "migrate to antigravity", fallback_minutes: 1440 },
    // cursor workspace quota (structured error event)
    QuotaSignature { agent: AgentKind::Cursor, needle: "quota exceeded for this workspace", fallback_minutes: 300 },
    // copilot CLI refusal when premium allowance is spent:
    // "You've reached your premium request limit for this billing cycle."
    QuotaSignature { agent: AgentKind::Copilot, needle: "premium request limit", fallback_minutes: 1440 },
];

/// True when a line is quoting this module's own signature table, not a live refusal.
pub(crate) fn is_signature_source_citation(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("quotasignature")
        || lower.contains("needle:")
        || lower.contains("fallback_minutes:")
        || lower.contains("rate_limit_signatures")
}

/// Match only signatures owned by one agent — used when scanning agent-authored
/// output so prose about another provider's quota cannot flip this run.
pub(crate) fn match_quota_signature_for_agent(
    message: &str,
    agent: AgentKind,
) -> Option<i64> {
    let lower = message.to_lowercase();
    QUOTA_SIGNATURES
        .iter()
        .find(|signature| signature.agent == agent && lower.contains(signature.needle))
        .map(|signature| signature.fallback_minutes)
}

/// Match a message against every provider signature. Returns the agent the
/// signature belongs to and its fallback cooldown, so a caller can both mark the
/// right agent and avoid the 5-minute default that expires while the provider is
/// still refusing work.
pub(crate) fn match_quota_signature(message: &str) -> Option<(AgentKind, i64)> {
    match_quota_signature_with_agent(message, None)
}

/// Prefer `preferred` when several agents share the same needle (e.g. Zen overlays).
pub(crate) fn match_quota_signature_with_agent(
    message: &str,
    preferred: Option<AgentKind>,
) -> Option<(AgentKind, i64)> {
    let lower = message.to_lowercase();
    if let Some(agent) = preferred {
        if let Some(minutes) = match_quota_signature_for_agent(message, agent) {
            return Some((agent, minutes));
        }
    }
    QUOTA_SIGNATURES
        .iter()
        .find(|signature| lower.contains(signature.needle))
        .map(|signature| (signature.agent, signature.fallback_minutes))
}

/// Parse relative reset phrasings that `parse_recovery_time`'s "try again at
/// <date>" format cannot reach: "resets in 1 day", "resets in 3 hours",
/// "5-hour quota". Returns an absolute local time.
pub(crate) fn parse_relative_recovery(message: &str) -> Option<NaiveDateTime> {
    let lower = message.to_lowercase();
    let now = Local::now().naive_local();

    if let Some(duration) = parse_compact_duration(&lower) {
        return Some(now + duration);
    }
    if let Some(duration) = parse_resets_in(&lower) {
        return Some(now + duration);
    }
    if let Some(at) = parse_reset_at_utc(&lower) {
        return Some(at);
    }
    if let Some(hours) = parse_hyphenated_hours(&lower) {
        return Some(now + Duration::hours(hours));
    }
    None
}

/// "the quota will reset at 08-05 15:12:00 utc" — an absolute instant with no
/// year, which is how qwen's token plan reports its window. Assumes the current
/// year and rolls to next year if that would land in the past.
fn parse_reset_at_utc(lower: &str) -> Option<NaiveDateTime> {
    let idx = lower.find("reset at ")?;
    let rest = lower[idx + "reset at ".len()..].trim();
    let stamp: String = rest.chars().take(14).collect();
    let year = Local::now().naive_local().year();
    let candidate =
        NaiveDateTime::parse_from_str(&format!("{year}-{stamp}"), "%Y-%m-%d %H:%M:%S").ok()?;
    let utc_offset = Local::now().offset().local_minus_utc();
    Some(candidate + Duration::seconds(i64::from(utc_offset)))
}

/// "resets in 59m21s" / "1h30m" — the compact form agy uses. The spaced parser
/// cannot read it, and falling through to a default cooldown turned a 59-minute
/// outage into a multi-hour one.
fn parse_compact_duration(lower: &str) -> Option<Duration> {
    let idx = lower.find("resets in ").or_else(|| lower.find("try again in "))?;
    let rest = lower[idx..].split_once(" in ")?.1.trim();
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, 'd' | 'h' | 'm' | 's'))
        .collect();
    if token.is_empty() || !token.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut total = Duration::zero();
    let mut number = String::new();
    for ch in token.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let amount: i64 = number.parse().ok()?;
        number.clear();
        total = total
            + match ch {
                'd' => Duration::days(amount),
                'h' => Duration::hours(amount),
                'm' => Duration::minutes(amount),
                's' => Duration::seconds(amount),
                _ => return None,
            };
    }
    (total > Duration::zero()).then_some(total)
}

/// "resets in 1 day" / "resets in 45 minutes" / "try again in 2 hours"
fn parse_resets_in(lower: &str) -> Option<Duration> {
    let idx = lower.find("resets in ").or_else(|| lower.find("try again in "))?;
    let rest = &lower[idx..];
    let rest = rest.split_once(" in ")?.1;
    let mut parts = rest.split_whitespace();
    let amount: i64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?.trim_end_matches([',', '.', ')']);
    unit_to_duration(unit, amount)
}

/// "5-hour quota" — the window length doubles as the wait when a provider does
/// not say when it resets.
fn parse_hyphenated_hours(lower: &str) -> Option<i64> {
    let idx = lower.find("-hour")?;
    let head = &lower[..idx];
    let digits: String = head.chars().rev().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse().ok()
}

fn unit_to_duration(unit: &str, amount: i64) -> Option<Duration> {
    match unit {
        u if u.starts_with("minute") || u == "min" || u == "mins" => Some(Duration::minutes(amount)),
        u if u.starts_with("hour") || u == "hr" || u == "hrs" => Some(Duration::hours(amount)),
        u if u.starts_with("day") => Some(Duration::days(amount)),
        u if u.starts_with("week") => Some(Duration::weeks(amount)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "rate_limit_signatures_tests.rs"]
mod tests;
