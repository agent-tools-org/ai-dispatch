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

pub(crate) fn candidates(kind: AgentKind) -> Vec<String> {
    let agent = super::get_agent(kind);
    let mut models = super::model_validation::get_served_models_cached(agent.as_ref())
        .unwrap_or_else(|| {
            super::model_group::groups_for_agent(kind)
                .iter()
                .flat_map(|(_, models)| models.iter().map(|model| (*model).to_string()))
                .collect()
        });
    models.retain(|model| crate::types::model_family(model) != "other");
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
        && crate::types::model_family(model) != "other"
        && !family_is_spent(kind, custom, crate::types::model_family(model))
    {
        return Some(model.to_string());
    }
    models
        .into_iter()
        .find(|model| !family_is_spent(kind, custom, crate::types::model_family(model)))
}

pub(crate) fn resolve(
    kind: AgentKind,
    custom: Option<&str>,
    model: Option<&str>,
    pinned: bool,
) -> anyhow::Result<Option<String>> {
    if !per_family(kind) {
        return Ok(model.map(str::to_string));
    }
    if pinned {
        let name = model.ok_or_else(|| anyhow::anyhow!("{} requires a model", kind.as_str()))?;
        let family = crate::types::model_family(name);
        anyhow::ensure!(family != "other", "Unknown quota family for model {name}");
        anyhow::ensure!(
            !family_is_spent(kind, custom, family),
            "{} model is held (family {family} spent); explicit pin retained",
            kind.as_str()
        );
        return Ok(Some(name.to_string()));
    }
    healthy_model(kind, custom, model)
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
    let current = current?;
    let family = crate::types::model_family(current);
    candidates(kind).into_iter().find(|model| {
        crate::types::model_family(model) != family
            && strength(model).0 >= strength(current).0
            && !family_is_spent(kind, custom, crate::types::model_family(model))
    })
}
