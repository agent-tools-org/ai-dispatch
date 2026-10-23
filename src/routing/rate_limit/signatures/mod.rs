// Per-CLI quota-exhaustion signatures and relative recovery-time parsing.
// Exports: agent-scoped quota signatures and provider-owned recovery clocks.
// Deps: types::AgentKind, chrono.

use crate::types::AgentKind;

mod cursor;
mod recovery;
pub(crate) use recovery::recovery_for_agent;

/// A provider's quota message, captured from a real run, plus how long that
/// quota class actually lasts. Provider-specific wording is matched separately
/// from generic transport errors.
pub(crate) struct QuotaSignature {
    pub(crate) agent: AgentKind,
    /// Lowercase CLI wording; Cursor's monthly error is anchored to a line start.
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
    /// No clock in the refusal. A dated live window may end it.
    /// A percentage without a date may not. `After` is not used as a floor:
    /// inventing a clock is how grok's 402 used to expire in five minutes.
    Windowed,
}

pub(crate) const QUOTA_SIGNATURES: &[QuotaSignature] = &[
    QuotaSignature { agent: AgentKind::Claude, needle: "you've hit your limit", recovery: QuotaRecovery::After(300) },
    QuotaSignature { agent: AgentKind::Claude, needle: "claude ai usage limit reached", recovery: QuotaRecovery::After(300) },
    // droid 0.183.0, captured 2026-08-05 as an HTTP 402 body:
    // "You've reached your weekly standard usage limit (resets in 1 day)."
    // A rolling window on a clock, and the message states the remainder.
    QuotaSignature { agent: AgentKind::Droid, needle: "weekly standard usage limit", recovery: QuotaRecovery::After(1440) },
    // The same limit on droid's shorter window, captured 2026-08-07 from the
    // marker droid wrote at 19:35: "You've reached your 5-hour standard usage
    // limit (resets in 1h 48min)." It states its own remainder, so `After` here
    // is only the floor for a message that ever omits it. Until this entry
    // existed the refusal was caught by the generic `402` rule alone — that is,
    // by the status code and not by anything droid said.
    QuotaSignature { agent: AgentKind::Droid, needle: "standard usage limit", recovery: QuotaRecovery::After(300) },
    // Droid weekly (2026-08-19) and monthly (2026-10-12) HTTP 402 bodies:
    // Both periods state a reset; waiting restores this pool and Extra Usage is optional.
    // A dated reset wins. The two-day fallback comes from both captures.
    QuotaSignature {
        agent: AgentKind::Droid,
        needle: "droid core usage limit",
        recovery: QuotaRecovery::After(2880),
    },
    // Cursor monthly cycle (captured 2026-09-11): dated reset, else 30 days.
    QuotaSignature { agent: AgentKind::Cursor, needle: cursor::NEEDLE, recovery: QuotaRecovery::After(43_200) },
    // codex-cli: "You have hit your usage limit ... try again at <date>."
    QuotaSignature { agent: AgentKind::Codex, needle: "hit your usage limit", recovery: QuotaRecovery::After(300) },
    // oz (Warp), captured 2026-08-05 exit 1: "Error: Quota limit reached."
    QuotaSignature { agent: AgentKind::Oz, needle: "quota limit reached", recovery: QuotaRecovery::After(60) },
    // oz stderr, captured 2026-09-14/15: logged-out runs die in 2-4s with this.
    QuotaSignature { agent: AgentKind::Oz, needle: "credentials are invalid", recovery: QuotaRecovery::NeedsHuman },
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
    // The same tier retirement, worded from the other side.
    QuotaSignature { agent: AgentKind::Antigravity, needle: "migrate to antigravity", recovery: QuotaRecovery::NeedsHuman },
    // cursor workspace quota (structured error event) — a window on a clock.
    QuotaSignature { agent: AgentKind::Cursor, needle: "quota exceeded for this workspace", recovery: QuotaRecovery::After(300) },
    // cursor premium pool spent, captured 2026-08-07 on t-dfc23e80, t-b38df7a8
    // and t-d6fef491:
    // "ActionRequiredError: Increase limits for faster responses You're out of
    //  usage. Switch to Auto, or ask your admin to increase your limit to continue."
    // A dated Plan window with headroom ends it. On-demand is not this pool.
    // `auto` is not held with it — see model_group::group_from_refusal.
    QuotaSignature { agent: AgentKind::Cursor, needle: "you're out of usage", recovery: QuotaRecovery::Windowed },
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
    // Qwen Code 0.21.5 ModelStudio token-plan refusals captured in result events.
    QuotaSignature { agent: AgentKind::Qwen, needle: "your token-plan 5-hour quota has been exhausted", recovery: QuotaRecovery::After(300) },
    QuotaSignature { agent: AgentKind::Qwen, needle: "your token-plan 1-week quota has been exhausted", recovery: QuotaRecovery::After(300) },
    // grok, captured 2026-08-07:
    // "API error (status 402 Payment Required): Grok Build usage balance exhausted"
    // Land now as Windowed: aidbar already maps the billing period to a dated
    // resets_at. Revert to NeedsHuman if a 402 arrives while the same-minute
    // aidbar snapshot still has headroom.
    QuotaSignature { agent: AgentKind::Grok, needle: "usage balance exhausted", recovery: QuotaRecovery::Windowed },
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
        .find(|signature| signature.agent == agent && cursor::matches(&lower, signature.needle))
        .map(|signature| signature.recovery)
}

#[cfg(test)]
#[path = "signatures_tests.rs"]
mod tests;
