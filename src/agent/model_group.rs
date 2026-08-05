// Model-group quota accounting for agents whose plan meters several families
// separately. Exports: model_group, groups_for_agent, has_grouped_quota.
// Deps: types::AgentKind.

use crate::types::AgentKind;

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
    matches!(agent, AgentKind::Antigravity)
}

/// The quota group a model belongs to, by family prefix. Returns None when the
/// agent meters its whole account together, so callers fall back to per-agent
/// marking unchanged.
pub(crate) fn model_group(agent: AgentKind, model: Option<&str>) -> Option<&'static str> {
    if !has_grouped_quota(agent) {
        return None;
    }
    let model = model?.to_ascii_lowercase();
    Some(family_of(&model))
}

fn family_of(model: &str) -> &'static str {
    if model.starts_with("gemini") {
        "gemini"
    } else if model.starts_with("claude") {
        "claude"
    } else if model.starts_with("gpt") {
        "gpt-oss"
    } else {
        "other"
    }
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
