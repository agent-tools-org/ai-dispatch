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

pub(crate) fn candidates(kind: AgentKind) -> Vec<String> {
    let agent = super::get_agent(kind);
    let mut models = super::model_validation::get_served_models_cached(agent.as_ref())
        .unwrap_or_else(|| {
            super::model_group::groups_for_agent(kind)
                .iter()
                .flat_map(|(_, models)| models.iter().map(|model| (*model).to_string()))
                .collect()
        });
    models.retain(|model| unit(kind, model) != "other");
    models.sort_by_key(|model| std::cmp::Reverse(strength(model)));
    models
}

fn strength(model: &str) -> (u8, u8) {
    let class = if model.contains("opus") || model.contains("argon") || model.contains("120b") {
        3
    } else if model.contains("sonnet") {
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
    models.into_iter().find(|model| !family_is_spent(kind, custom, unit(kind, model)))
}

fn select_model(
    kind: AgentKind,
    custom: Option<&str>,
    current: Option<&str>,
    budget: bool,
    excluded: Option<&str>,
) -> Option<String> {
    let effort = current.map(strength).map_or(2, |rank| rank.1);
    let mut models = candidates(kind);
    models.sort_by_key(|model| std::cmp::Reverse((strength(model).1 == effort, strength(model))));
    if budget {
        models.sort_by_key(|model| (strength(model).0, std::cmp::Reverse(strength(model).1)));
    }
    models.into_iter().find(|model| {
        Some(model.as_str()) != excluded
            && strength(model).1 <= effort
            && !family_is_spent(kind, custom, unit(kind, model))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinPolicy {
    Automatic,
    WarnOnReplacement,
    Exact,
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
        let name = model.ok_or_else(|| anyhow::anyhow!("{} requires a model", kind.as_str()))?;
        let family = unit(kind, name);
        anyhow::ensure!(family != "other", "Unknown quota family for model {name}");
        if Some(name) != excluded && !family_is_spent(kind, custom, family) {
            return Ok(Some(name.to_string()));
        }
        anyhow::ensure!(
            policy != PinPolicy::Exact,
            "{} model is held (family {family} spent); exact pin retained",
            kind.as_str()
        );
        let replacement = select_model(kind, custom, model, budget, excluded)
            .ok_or_else(|| anyhow::anyhow!("{} has no unspent model family", kind.as_str()))?;
        aid_warn!(
            "[aid] {} model group exhausted; switching {} -> {}",
            kind.as_str(),
            name,
            replacement
        );
        return Ok(Some(replacement));
    }
    let current = model.filter(|name| {
        !budget && Some(*name) != excluded
            && candidates(kind).iter().any(|served| served == name)
            && !family_is_spent(kind, custom, unit(kind, name))
    });
    current.map(str::to_string)
        .or_else(|| select_model(kind, custom, model, budget, excluded))
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("{} has no unspent model family", kind.as_str()))
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
    candidates(kind).into_iter().find(|model| {
        Some(unit(kind, model)) != family
            && strength(model).0 >= minimum_class
            && strength(model).1 <= current.map(strength).map_or(2, |rank| rank.1)
            && !family_is_spent(kind, custom, unit(kind, model))
    })
}
