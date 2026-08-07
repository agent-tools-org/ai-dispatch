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
    /// What ends this refusal.
    pub(crate) recovery: QuotaRecovery,
}

/// What ends a refusal — a clock, or a person.
///
/// These are different facts and collapsing them loses a route in one direction
/// or the other. Giving a spent balance a cooldown hands work back to an account
/// that still cannot pay; giving a transient 429 a permanent hold writes off a
/// route that is already serving again. Encoding the class here means each
/// signature states which one it is on its own evidence, instead of a magic
/// minute count standing in for "never".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuotaRecovery {
    /// A clock ends it. The value is the cooldown to apply when the message
    /// itself carries no parseable reset time.
    ///
    /// A wrong-but-short guess is worse than none: it sends work back to a
    /// provider that is still exhausted.
    After(i64),
    /// Only a person ends it: a top-up, a plan change, an admin raising a
    /// limit, or a billing cycle whose date the message does not state. Any
    /// number of minutes chosen here would be invented and would expire while
    /// the provider is still refusing, so the marker holds until
    /// `aid config clear-limit <agent>`.
    NeedsHuman,
}

pub(crate) const QUOTA_SIGNATURES: &[QuotaSignature] = &[
    // qwen 0.21.5, ModelStudio token plan, captured 2026-08-05:
    // "Quota exhausted: Your token-plan 5-hour quota has been exhausted."
    QuotaSignature { agent: AgentKind::Qwen, needle: "quota has been exhausted", recovery: QuotaRecovery::After(300) },
    QuotaSignature { agent: AgentKind::Qwen, needle: "quota exhausted", recovery: QuotaRecovery::After(300) },
    // droid 0.183.0, captured 2026-08-05 as an HTTP 402 body:
    // "You've reached your weekly standard usage limit (resets in 1 day)."
    // A rolling window on a clock, and the message states the remainder.
    QuotaSignature { agent: AgentKind::Droid, needle: "weekly standard usage limit", recovery: QuotaRecovery::After(1440) },
    // codex-cli, captured previously:
    // "You have hit your usage limit ... try again at <date>."
    QuotaSignature { agent: AgentKind::Codex, needle: "hit your usage limit", recovery: QuotaRecovery::After(300) },
    // oz (Warp cloud agents), captured 2026-08-05 with exit code 1:
    // "Error: Quota limit reached."
    // No reset time is given at all, so the cooldown is a guess; an hour keeps
    // the agent out of rotation without writing it off for the day.
    QuotaSignature { agent: AgentKind::Oz, needle: "quota limit reached", recovery: QuotaRecovery::After(60) },
    // agy 1.1.10, captured 2026-08-05 against the gemini group while the claude
    // group was still serving:
    // "Individual quota reached. Please upgrade your subscription to increase
    //  your limits. Resets in 59m21s."
    // The earlier entry here used the bare needle "quota" with an invented
    // 12-hour cooldown; it matched unrelated output and would have stranded a
    // working claude allowance for twelve hours over a 59-minute gemini outage.
    QuotaSignature { agent: AgentKind::Antigravity, needle: "individual quota reached", recovery: QuotaRecovery::After(60) },
    // opencode Zen, captured 2026-08-05 from t-76181278 as an HTTP 401 body:
    // {"type":"error","error":{"name":"APIError","data":{"message":"Insufficient
    //  balance. Manage your billing here: ...","statusCode":401}}}
    //
    // Unlike the entries above this is not a time-based quota: it ends when the
    // account is topped up, never on a clock. It previously carried a one-day
    // cooldown only because the table could not say "a person ends this"; it can
    // now, and `aid config clear-limit opencode` is the escape hatch after paying.
    //
    // Neither the generic phrase list nor a status-code check caught this: 401
    // is neither 429 nor 402, and no needle contained "insufficient balance", so
    // aid kept reporting opencode as OK and kept dispatching to it.
    QuotaSignature { agent: AgentKind::OpenCode, needle: "insufficient balance", recovery: QuotaRecovery::NeedsHuman },
    // OpenCode-compatible overlays share the same Zen billing refusal shape.
    QuotaSignature { agent: AgentKind::MiMoCode, needle: "insufficient balance", recovery: QuotaRecovery::NeedsHuman },
    QuotaSignature { agent: AgentKind::Kilo, needle: "insufficient balance", recovery: QuotaRecovery::NeedsHuman },
    // droid 0.183.0, captured as HTTP 402 body:
    // "402 payment required: reload your tokens"
    // Reloading tokens is a purchase, not a window elapsing — distinct from
    // droid's weekly usage limit above, which does run on a clock.
    QuotaSignature { agent: AgentKind::Droid, needle: "reload your tokens", recovery: QuotaRecovery::NeedsHuman },
    // gemini Code Assist, captured 2026-08-05:
    // "IneligibleTierError: ... migrate to Antigravity"
    // Not a quota at all: this tier is no longer served by this CLI and no
    // amount of waiting restores it. A person migrates to agy.
    QuotaSignature { agent: AgentKind::Gemini, needle: "ineligibletier", recovery: QuotaRecovery::NeedsHuman },
    // Per-window API quota on the paid endpoint; this one does refill on a clock.
    QuotaSignature { agent: AgentKind::Gemini, needle: "resource exhausted", recovery: QuotaRecovery::After(60) },
    QuotaSignature { agent: AgentKind::Gemini, needle: "resourceexhausted", recovery: QuotaRecovery::After(60) },
    // The same tier retirement, worded from the other side.
    QuotaSignature { agent: AgentKind::Antigravity, needle: "migrate to antigravity", recovery: QuotaRecovery::NeedsHuman },
    // cursor workspace quota (structured error event) — a window on a clock.
    QuotaSignature { agent: AgentKind::Cursor, needle: "quota exceeded for this workspace", recovery: QuotaRecovery::After(300) },
    // cursor premium pool spent, captured 2026-08-07 on t-dfc23e80, t-b38df7a8
    // and t-d6fef491:
    // "ActionRequiredError: Increase limits for faster responses You're out of
    //  usage. Switch to Auto, or ask your admin to increase your limit to continue."
    // The message names the two ways out and both are human actions. `auto` is
    // not held with it — see model_group::model_group.
    QuotaSignature { agent: AgentKind::Cursor, needle: "you're out of usage", recovery: QuotaRecovery::NeedsHuman },
    // copilot CLI refusal when premium allowance is spent:
    // "You've reached your premium request limit for this billing cycle."
    // This does return at the next billing cycle, but the message never says
    // when that is. The old one-day cooldown expired weeks early and handed work
    // back to a spent allowance, so the hold waits for a person instead.
    QuotaSignature { agent: AgentKind::Copilot, needle: "premium request limit", recovery: QuotaRecovery::NeedsHuman },
    // copilot, captured 2026-08-07 on t-03a68876 and t-80cf4b62 as a JSON event:
    // {"errorCode":"quota_exceeded","message":"You have exceeded your monthly quota"}
    // Same class as above: monthly, with no stated reset date.
    QuotaSignature { agent: AgentKind::Copilot, needle: "exceeded your monthly quota", recovery: QuotaRecovery::NeedsHuman },
    // grok, captured 2026-08-07:
    // "API error (status 402 Payment Required): Grok Build usage balance exhausted"
    // A spent balance does not come back on a clock. Before this entry existed
    // the refusal matched only the generic 402 rule, which wrote a marker with
    // no recovery time that stopped counting after five minutes.
    QuotaSignature { agent: AgentKind::Grok, needle: "usage balance exhausted", recovery: QuotaRecovery::NeedsHuman },
];

/// Match only signatures owned by one agent, so a refusal quoted about another
/// provider cannot flip this run.
pub(crate) fn match_quota_signature_for_agent(
    message: &str,
    agent: AgentKind,
) -> Option<QuotaRecovery> {
    let lower = message.to_lowercase();
    QUOTA_SIGNATURES
        .iter()
        .find(|signature| signature.agent == agent && lower.contains(signature.needle))
        .map(|signature| signature.recovery)
}

/// Match a message against every provider signature. Returns the agent the
/// signature belongs to and what ends the refusal, so a caller can both mark the
/// right agent and avoid the 5-minute default that expires while the provider is
/// still refusing work.
pub(crate) fn match_quota_signature(message: &str) -> Option<(AgentKind, QuotaRecovery)> {
    match_quota_signature_with_agent(message, None)
}

/// Prefer `preferred` when several agents share the same needle (e.g. Zen overlays).
pub(crate) fn match_quota_signature_with_agent(
    message: &str,
    preferred: Option<AgentKind>,
) -> Option<(AgentKind, QuotaRecovery)> {
    let lower = message.to_lowercase();
    if let Some(agent) = preferred {
        if let Some(recovery) = match_quota_signature_for_agent(message, agent) {
            return Some((agent, recovery));
        }
    }
    QUOTA_SIGNATURES
        .iter()
        .find(|signature| lower.contains(signature.needle))
        .map(|signature| (signature.agent, signature.recovery))
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
