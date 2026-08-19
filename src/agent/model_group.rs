// Model-group quota accounting for agents whose plan meters several families
// separately. Exports: model_group, group_from_refusal, groups_for_agent,
// has_grouped_quota.
// Deps: types::AgentKind.

use crate::types::AgentKind;

/// Cursor's metered premium pool, and the `auto` tier that outlives it.
const PREMIUM_GROUP: &str = "premium";
const AUTO_GROUP: &str = "auto";

/// Factory's weekly/5-hour standard pool, and the Core models that outlive it.
const STANDARD_GROUP: &str = "standard";
const CORE_GROUP: &str = "core";

/// Confirmed Droid Core ids only. Unlisted names are standard — enumerating
/// the spent pool instead left every other standard model dispatchable.
const DROID_CORE: &[&str] = &[
    "glm-5.2",
    "kimi-k3",
    "minimax-m3",
    "deepseek-v4-flash-0731",
];

/// Agents whose quota is metered per model family rather than per account.
///
/// agy is the case that forced this: `agy models` serves gemini-*, claude-* and
/// gpt-oss-* families, and each is metered on its own allowance. Captured
/// 2026-08-05 within one minute of each other:
///
/// ```text
/// $ agy -p "Reply OK" --model gemini-3.6-flash-low
/// Error: Individual quota reached. ... Resets in 59m21s.
/// $ agy -p "Reply OK" --model claude-sonnet-4-6
/// OK
/// ```
///
/// Marking the whole agent rate-limited on the first message would strand a
/// working claude allowance and hand the work to a weaker agent — the mirror of
/// the failures-reported-as-success class: a usable resource reported as dead.
pub(crate) fn has_grouped_quota(agent: AgentKind) -> bool {
    // Two independent reasons an allowance is partitioned, and they are not the
    // same fact. A provider that meters per model family says so in the
    // provider table, and a second such provider needs no change here at all.
    //
    // A tier split *inside* one pool is different: cursor is a subscription —
    // one account, one bill — but `auto` keeps serving after the premium pool is
    // spent, so the two must be marked separately. That is not a metering shape,
    // and rewriting cursor's shape to `PerModelFamily` to obtain grouping here
    // is what broke cost classification and needed an agent-specific patch in
    // the pricing layer to undo. The group table states the split directly.
    if agent == AgentKind::OpenCode || !groups_for_agent(agent).is_empty() {
        return true;
    }
    matches!(
        crate::types::provider_for_cli(agent).1,
        crate::types::MeteringShape::PerModelFamily
    )
}

/// The quota group a model belongs to, by family prefix. Returns None when the
/// agent meters its whole account together, so callers fall back to per-agent
/// marking unchanged.
pub(crate) fn model_group<'a>(agent: AgentKind, model: Option<&'a str>) -> Option<&'a str> {
    if agent == AgentKind::OpenCode {
        return model.and_then(provider_from_model);
    }
    if agent == AgentKind::Droid {
        return Some(droid_group(model));
    }
    if !has_grouped_quota(agent) {
        return None;
    }
    let model = model?.to_ascii_lowercase();
    if agent == AgentKind::Cursor {
        // Cursor meters one shared premium pool and `auto` is the only thing
        // that keeps serving once it is spent, so `auto` is the exception and
        // everything else draws on the metered pool.
        //
        // Enumerating premium models instead gets this backwards: the first
        // version of this listed `composer-2.5` and `gpt-5.4-high` because those
        // were the two models in the day's evidence, which left every other
        // premium model reading as unmetered and dispatchable after the pool was
        // already gone.
        return Some(if model.starts_with("auto") { AUTO_GROUP } else { PREMIUM_GROUP });
    }
    Some(family_of(&model))
}

fn droid_group(model: Option<&str>) -> &'static str {
    match model {
        Some(name) if is_droid_core(name) => CORE_GROUP,
        _ => STANDARD_GROUP,
    }
}

fn is_droid_core(model: &str) -> bool {
    DROID_CORE
        .iter()
        .any(|id| model.eq_ignore_ascii_case(id))
}

/// The group a refusal names, for providers whose own wording identifies the
/// exhausted tier.
///
/// Most marker writes happen where no model is in hand: a stderr line, a stream
/// error event, a failed task's captured output. Those paths marked the whole
/// agent, which for cursor meant one premium refusal took `auto` out with it —
/// `auto` being the one tier that keeps serving once the premium pool is spent.
/// The refusal itself says so, so it is read here rather than guessed.
pub(crate) fn group_from_refusal<'a>(agent: AgentKind, message: &'a str) -> Option<&'a str> {
    if agent == AgentKind::OpenCode {
        return named_opencode_provider(message);
    }
    if agent == AgentKind::Droid {
        // Weekly and 5-hour refusals both name the spent tier; a 402 that
        // names no tier (reload-your-tokens) stays agent-wide.
        return message
            .to_ascii_lowercase()
            .contains("standard usage")
            .then_some(STANDARD_GROUP);
    }
    if agent != AgentKind::Cursor {
        return None;
    }
    // The needle is cursor's premium signature verbatim; its other quota
    // refusal — "quota exceeded for this workspace" — names no tier and stays
    // agent-wide, which is right: a workspace cap is not a tier cap.
    message
        .to_ascii_lowercase()
        .contains("you're out of usage")
        .then_some(PREMIUM_GROUP)
}

/// Read OpenCode's provider attribution from the parsed error envelope.
pub(crate) fn group_from_refusal_value<'a>(
    agent: AgentKind,
    value: &'a serde_json::Value,
) -> Option<&'a str> {
    if agent != AgentKind::OpenCode {
        return None;
    }
    ["providerID", "provider_id", "provider", "model"]
        .iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .and_then(|value| provider_from_model(value).or(Some(value)))
        .filter(|provider| !provider.eq_ignore_ascii_case("unknown"))
}

fn provider_from_model(model: &str) -> Option<&str> {
    let (provider, _) = model.split_once('/')?;
    (!provider.is_empty()).then_some(provider)
}

fn named_opencode_provider(message: &str) -> Option<&str> {
    let lower = message.to_ascii_lowercase();
    ["providerid", "provider_id", "provider", "model"]
        .iter()
        .find_map(|key| value_after_key(message, &lower, key))
        .and_then(|value| provider_from_model(value).or(Some(value)))
        .filter(|provider| !provider.eq_ignore_ascii_case("unknown"))
}

fn value_after_key<'a>(message: &'a str, lower: &str, key: &str) -> Option<&'a str> {
    let start = lower.find(key)? + key.len();
    let value = message[start..].split_once(':')?.1.trim_start();
    let value = value.strip_prefix('"').unwrap_or(value);
    let end = value
        .find(|ch: char| ch == '"' || ch == ',' || ch == '}' || ch.is_whitespace())
        .unwrap_or(value.len());
    (!value[..end].is_empty()).then_some(&value[..end])
}

/// Delegates to the types layer: how a provider partitions its allowance is a
/// fact about the provider, and keeping a second copy here had already produced
/// two different answers for `gpt-*`.
fn family_of(model: &str) -> &'static str {
    crate::types::model_family(model)
}

/// Every group an agent can draw on, most capable first within each family.
/// Used to pick a replacement when the group in use is exhausted.
pub(crate) fn groups_for_agent(agent: AgentKind) -> &'static [(&'static str, &'static [&'static str])] {
    match agent {
        // Captured from `agy models`, 2026-08-05.
        AgentKind::Antigravity => &[
            ("gemini", &["gemini-3.1-pro-high", "gemini-3.6-flash-high", "gemini-3.6-flash-low"]),
            ("claude", &["claude-opus-4-6-thinking", "claude-sonnet-4-6"]),
            ("gpt-oss", &["gpt-oss-120b-medium"]),
        ],
        // Cursor's two tiers. The model lists are a preference order for
        // picking a replacement, not the membership test — `model_group`
        // decides membership, and it treats everything that is not `auto` as
        // premium so an unlisted premium model is still held.
        AgentKind::Cursor => &[
            (PREMIUM_GROUP, &["composer-2.5", "gpt-5.4-high"]),
            (AUTO_GROUP, &["auto"]),
        ],
        // Preference order for a replacement, not membership — `model_group`
        // treats only the Core allowlist as `core`.
        AgentKind::Droid => &[
            (STANDARD_GROUP, &["claude-opus-5"]),
            (
                CORE_GROUP,
                &[
                    "glm-5.2",
                    "kimi-k3",
                    "minimax-m3",
                    "deepseek-v4-flash-0731",
                ],
            ),
        ],
        _ => &[],
    }
}

/// First model from a group that is not exhausted, preferring the caller's
/// current group so an unrelated quota outage never moves work off it.
pub(crate) fn healthy_model_for(
    agent: AgentKind,
    current: Option<&str>,
    is_group_limited: impl Fn(&str) -> bool,
) -> Option<&'static str> {
    let groups = groups_for_agent(agent);
    if groups.is_empty() {
        return None;
    }
    let current_group = model_group(agent, current);
    if let Some(group) = current_group
        && !is_group_limited(group)
    {
        return None; // current model is fine; do not churn
    }
    groups
        .iter()
        .find(|(name, models)| !is_group_limited(name) && !models.is_empty())
        .and_then(|(_, models)| models.first().copied())
}

#[cfg(test)]
#[path = "model_group_tests.rs"]
mod tests;
