// Model-group quota accounting for agents whose plan meters several families
// separately. Exports: model_group, groups_for_agent, has_grouped_quota.
// Deps: types::AgentKind.

use crate::types::AgentKind;

/// Cursor's metered premium pool, and the `auto` tier that outlives it.
const PREMIUM_GROUP: &str = "premium";
const AUTO_GROUP: &str = "auto";

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
    if !groups_for_agent(agent).is_empty() {
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
pub(crate) fn model_group(agent: AgentKind, model: Option<&str>) -> Option<&'static str> {
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
