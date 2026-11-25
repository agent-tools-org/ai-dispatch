// Family-sized advice inventory and observed-model history attribution.
// Reuses dispatch quota membership and the served-model cache; never probes.
use super::*;
use crate::agent::model_group::{groups_for_agent, model_group};
use crate::types::{MeteringShape, TaskFilter, TaskStatus, provider_for_cli};

fn grouped(kind: AgentKind) -> bool {
    provider_for_cli(kind).1 == MeteringShape::PerModelFamily
}

pub(super) fn models(kind: AgentKind, category: TaskCategory) -> Vec<Option<String>> {
    if !grouped(kind) {
        return vec![None];
    }
    let served = crate::agent::model_validation::load_from_disk_cache(kind).unwrap_or_else(|| {
        crate::routing::model_catalog::static_models_for_agent(&kind)
            .into_iter()
            .map(|row| row.model.to_string())
            .collect()
    });
    let mut families = std::collections::BTreeMap::<String, String>::new();
    for model in served {
        let Some(group) = model_group(kind, Some(&model)) else {
            continue;
        };
        let replace = families.get(group).is_none_or(|current| {
            strength(kind, &model, category) > strength(kind, current, category)
        });
        if replace {
            families.insert(group.to_string(), model);
        }
    }
    families.into_values().map(Some).collect()
}

fn strength(kind: AgentKind, model: &str, category: TaskCategory) -> (i64, i64, i32, String) {
    let score = model_capability_score(kind, model, category)
        .map(|value| (value * 1_000.0) as i64)
        .unwrap_or(-1);
    let preference = groups_for_agent(kind)
        .iter()
        .flat_map(|(_, models)| models.iter())
        .position(|id| *id == model)
        .map(|index| -(index as i64))
        .unwrap_or(i64::MIN);
    let effort = if model.ends_with("-high") {
        2
    } else if model.ends_with("-medium") {
        1
    } else {
        0
    };
    (score, preference, effort, model.to_string())
}

pub(super) fn attribute_history(
    ranked: &mut [RankedCandidate],
    store: Option<&Store>,
    kind: TaskCategory,
    costs: &HashMap<AgentKind, f64>,
) {
    let tasks = store
        .and_then(|store| store.list_tasks(TaskFilter::All).ok())
        .unwrap_or_default();
    for item in ranked.iter_mut().filter(|item| grouped(item.order.kind)) {
        let agent = item.order.kind;
        let group = model_group(agent, item.report.model.as_deref());
        let matching: Vec<_> = tasks
            .iter()
            .filter(|task| {
                task.agent == agent
                    && task.attributed_model().is_some()
                    && model_group(agent, task.attributed_model()) == group
                    && matches!(
                        task.status,
                        TaskStatus::Done | TaskStatus::Merged | TaskStatus::Failed
                    )
                    && !task.outcome().is_unverified()
            })
            .collect();
        let bonus = history_bonus(&matching, kind);
        let delta = bonus - item.report.breakdown.history_bonus;
        item.report.breakdown.history_bonus = bonus;
        item.report.breakdown.total += delta;
        item.report.score += delta;
        item.order.score += delta;
        let cost = costs.get(&agent).copied().unwrap_or(0.0);
        item.order.efficiency = cost_efficiency(item.order.score, cost);
    }
}

fn history_bonus(matching: &[&crate::types::Task], kind: TaskCategory) -> f64 {
    let category: Vec<_> = matching
        .iter()
        .filter(|task| task.category.as_deref() == Some(kind.label()))
        .copied()
        .collect();
    let history: &[&crate::types::Task] = if category.len() >= 5 {
        &category
    } else {
        matching
    };
    if history.len() >= 5 {
        let successes = history
            .iter()
            .filter(|task| task.outcome().is_success())
            .count();
        ((successes as f64 / history.len() as f64 - 0.75) * 16.0)
            .round()
            .clamp(-5.0, 4.0)
    } else {
        0.0
    }
}
