// Concrete model selection for providers metered by family.
// Uses cached served models and shared family quota evidence before dispatch.
use crate::routing::availability::family_is_spent;
use crate::types::{AgentKind, MeteringShape};

pub(crate) fn per_family(kind: AgentKind) -> bool {
    matches!(
        crate::types::provider_for_cli(kind).1,
        MeteringShape::PerModelFamily
    )
}

fn unit(kind: AgentKind, model: &str) -> &str {
    super::model_group::model_group(kind, Some(model)).unwrap_or("other")
}

/// Served models with a known quota unit, strongest first. A served list with no
/// recognizable model (missing, or a CLI that answers `models` with something else)
/// falls back to the static table rather than reading as "every unit spent".
pub(crate) fn candidates(kind: AgentKind) -> Vec<String> {
    let agent = super::get_agent(kind);
    let served = super::model_validation::get_served_models_cached(agent.as_ref());
    let mut models = known_units(kind, served.unwrap_or_default());
    if models.is_empty() {
        models = known_units(kind, static_models(kind));
    }
    models.sort_by_key(|model| std::cmp::Reverse(strength(model)));
    models
}

fn known_units(kind: AgentKind, mut models: Vec<String>) -> Vec<String> {
    models.retain(|model| unit(kind, model) != "other");
    models
}

fn static_models(kind: AgentKind) -> Vec<String> {
    super::model_group::groups_for_agent(kind)
        .iter()
        .flat_map(|(_, models)| models.iter().map(|model| (*model).to_string()))
        .collect()
}

/// Replacement order: models from the caller's vendor first, so a quota switch
/// never silently trades a Gemini review for a Claude one. Stable within a vendor.
fn same_vendor_first(mut models: Vec<String>, current: Option<&str>) -> Vec<String> {
    if let Some(current) = current {
        let vendor = crate::types::model_family(current);
        models.sort_by_key(|model| crate::types::model_family(model) != vendor);
    }
    models
}

fn strength(model: &str) -> (u8, u8) {
    let flagship = ["opus", "argon", "neon", "120b"];
    let class = if flagship.iter().any(|name| model.contains(name)) {
        3
    } else if model.contains("sonnet") || model.contains("hydrogen") {
        2
    } else {
        1
    };
    let effort = if model.ends_with("high") {
        3
    } else if model.ends_with("medium") {
        2
    } else {
        1
    };
    (class, effort)
}

pub(crate) fn healthy_model(
    kind: AgentKind,
    custom: Option<&str>,
    current: Option<&str>,
) -> Option<String> {
    let models = candidates(kind);
    if let Some(model) = current
        && models.iter().any(|served| served == model)
        && unit(kind, model) != "other"
        && !family_is_spent(kind, custom, unit(kind, model))
    {
        return Some(model.to_string());
    }
    models
        .into_iter()
        .find(|model| !family_is_spent(kind, custom, unit(kind, model)))
}

fn select_model(
    kind: AgentKind,
    custom: Option<&str>,
    current: Option<&str>,
    budget: bool,
    excluded: Option<&str>,
    exclude_pool: bool,
) -> Option<String> {
    let effort = current.map(strength).map_or(2, |rank| rank.1);
    let mut models = candidates(kind);
    if budget {
        models.sort_by_key(|model| (strength(model).0, std::cmp::Reverse(strength(model).1)));
    }
    same_vendor_first(models, current).into_iter().find(|model| {
        !excluded.is_some_and(|failed| {
            model == failed || (exclude_pool && unit(kind, model) == unit(kind, failed))
        }) && strength(model).1 == effort
            && !family_is_spent(kind, custom, unit(kind, model))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinPolicy {
    Automatic,
    WarnOnReplacement,
    Exact,
    Continuation,
}

pub(crate) fn resolve(
    kind: AgentKind,
    custom: Option<&str>,
    model: Option<&str>,
    policy: PinPolicy,
    budget: bool,
    excluded: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if !per_family(kind) {
        return Ok(model.map(str::to_string));
    }
    if policy != PinPolicy::Automatic {
        return resolve_pin(kind, custom, model, policy, budget, excluded).map(Some);
    }
    let current = model.filter(|name| {
        !budget
            && Some(*name) != excluded
            && candidates(kind).iter().any(|served| served == name)
            && !family_is_spent(kind, custom, unit(kind, name))
    });
    current
        .map(str::to_string)
        .or_else(|| select_model(kind, custom, model, budget, excluded, false))
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("{} has no unspent model family", kind.as_str()))
}

fn resolve_pin(
    kind: AgentKind,
    custom: Option<&str>,
    model: Option<&str>,
    policy: PinPolicy,
    budget: bool,
    excluded: Option<&str>,
) -> anyhow::Result<String> {
    let name = model.ok_or_else(|| anyhow::anyhow!("{} requires a model", kind.as_str()))?;
    let family = unit(kind, name);
    anyhow::ensure!(family != "other", "Unknown quota family for model {name}");
    let excluded_pin = excluded.is_some_and(|failed| {
        name == failed || (policy == PinPolicy::Continuation && family == unit(kind, failed))
    });
    if !excluded_pin && !family_is_spent(kind, custom, family) {
        return Ok(name.to_string());
    }
    anyhow::ensure!(
        policy != PinPolicy::Exact,
        "{} model is held (family {family} spent); exact pin retained",
        kind.as_str()
    );
    let replacement = select_model(
        kind,
        custom,
        model,
        budget,
        excluded,
        policy == PinPolicy::Continuation,
    )
    .ok_or_else(|| anyhow::anyhow!("{} has no unspent model family", kind.as_str()))?;
    aid_warn!(
        "[aid] {} model group exhausted; switching {} -> {}",
        kind.as_str(),
        name,
        replacement
    );
    Ok(replacement)
}

pub(crate) fn continuation_model(
    kind: AgentKind,
    custom: Option<&str>,
    current: Option<&str>,
) -> Option<String> {
    if !per_family(kind) {
        return None;
    }
    let family = current.map(|model| unit(kind, model));
    let minimum_class = current.map(strength).map_or(0, |rank| rank.0);
    same_vendor_first(candidates(kind), current).into_iter().find(|model| {
        Some(unit(kind, model)) != family
            && strength(model).0 >= minimum_class
            && strength(model).1 <= current.map(strength).map_or(2, |rank| rank.1)
            && !family_is_spent(kind, custom, unit(kind, model))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_continuation_excludes_sibling_model_in_refused_pool() {
        let home = crate::support::test_env::TestHome::new();
        let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
        let _models = super::super::model_validation::MockServedModelsGuard::set(
            AgentKind::Antigravity,
            Some(vec![
                "gpt-oss-120b-medium".into(),
                "gemini-4-argon-medium".into(),
            ]),
        );
        let result = resolve(
            AgentKind::Antigravity,
            None,
            Some("gpt-oss-120b-medium"),
            PinPolicy::Continuation,
            true,
            Some("claude-opus-5-5-medium"),
        );
        assert_eq!(
            result.expect("another pool"),
            Some("gemini-4-argon-medium".into())
        );
    }

    /// Served list as agy reported it on 2026-11-22, Argon pool marked spent.
    fn argon_spent_with_served_gemini_4() -> super::super::model_validation::MockServedModelsGuard {
        let guard = super::super::model_validation::MockServedModelsGuard::set(
            AgentKind::Antigravity,
            Some(
                [
                    "gemini-4-neon-high", "gemini-4-argon-high", "gemini-4-hydrogen-high",
                    "gemini-3.8-flash-high", "claude-opus-5-5-high", "gpt-oss-120b-medium",
                ]
                .map(String::from)
                .to_vec(),
            ),
        );
        crate::routing::rate_limit::mark_untrusted_for_model(
            &AgentKind::Antigravity,
            None,
            Some("gemini-4-argon-high"),
            "Individual quota reached. Resets in 108h2m41s.",
        );
        guard
    }

    #[test]
    fn agy_spent_argon_pin_switches_to_gemini_4_neon_not_claude() {
        let home = crate::support::test_env::TestHome::new();
        let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
        let _served = argon_spent_with_served_gemini_4();
        for policy in [PinPolicy::WarnOnReplacement, PinPolicy::Continuation] {
            let excluded = (policy == PinPolicy::Continuation).then_some("gemini-4-argon-high");
            let result = resolve(
                AgentKind::Antigravity, None, Some("gemini-4-argon-high"), policy, false, excluded,
            );
            assert_eq!(result.expect("replacement"), Some("gemini-4-neon-high".into()));
        }
        assert_eq!(
            continuation_model(AgentKind::Antigravity, None, Some("gemini-4-argon-high")),
            Some("gemini-4-neon-high".into())
        );
    }
}
