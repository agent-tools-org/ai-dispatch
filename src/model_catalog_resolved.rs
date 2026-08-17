// Owned model-catalog records, including CLI-discovered models and pricing overrides.
// Exports resolved catalog queries without adding a second discovery cache.
// Deps: static model catalog, served-model disk cache, serde.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

use super::{static_models_for_agent, AgentModel, AGENT_MODELS};
use crate::types::AgentKind;

#[derive(Debug, Clone, Deserialize)]
pub struct PricingFileModel {
    pub agent: String,
    pub model: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub tier: String,
    pub description: String,
    pub updated: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedAgentModel {
    pub agent: AgentKind,
    pub model: String,
    pub input_per_m: Option<f64>,
    pub output_per_m: Option<f64>,
    pub tier: String,
    pub description: String,
    pub capability: Option<f64>,
}

impl From<&AgentModel> for ResolvedAgentModel {
    fn from(model: &AgentModel) -> Self {
        Self {
            agent: model.agent,
            model: model.model.to_string(),
            input_per_m: Some(model.input_per_m),
            output_per_m: Some(model.output_per_m),
            tier: model.tier.to_string(),
            description: model.description.to_string(),
            capability: Some(model.capability),
        }
    }
}

impl ResolvedAgentModel {
    pub fn from_override(agent: AgentKind, model: PricingFileModel) -> Self {
        let PricingFileModel {
            model, input_per_m, output_per_m, tier, description, updated, ..
        } = model;
        let _ = updated;
        Self {
            agent,
            model,
            input_per_m: Some(input_per_m),
            output_per_m: Some(output_per_m),
            tier,
            description,
            capability: None,
        }
    }

    pub fn apply_override(&mut self, model: PricingFileModel) {
        let PricingFileModel {
            input_per_m, output_per_m, tier, description, updated, ..
        } = model;
        let _ = updated;
        self.input_per_m = Some(input_per_m);
        self.output_per_m = Some(output_per_m);
        self.tier = tier;
        self.description = description;
    }
}

pub fn models_for_agent(agent: &AgentKind) -> Vec<ResolvedAgentModel> {
    let mut models: Vec<_> = static_models_for_agent(agent)
        .into_iter()
        .map(ResolvedAgentModel::from)
        .collect();
    if *agent == AgentKind::Antigravity {
        models.extend(discovered_agy_models());
    }
    models
}

fn discovered_agy_models() -> Vec<ResolvedAgentModel> {
    crate::agent::model_validation::load_from_disk_cache(AgentKind::Antigravity)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| {
            !AGENT_MODELS.iter().any(|known| {
                known.agent == AgentKind::Antigravity && known.model.eq_ignore_ascii_case(name)
            })
        })
        .map(|model| ResolvedAgentModel {
            agent: AgentKind::Antigravity,
            model,
            input_per_m: None,
            output_per_m: None,
            tier: "unknown".to_string(),
            description: "Discovered from agy; pricing and capability unknown".to_string(),
            capability: None,
        })
        .collect()
}

pub(crate) fn is_unpriced_discovered_model(agent: AgentKind, model: &str) -> bool {
    agent == AgentKind::Antigravity
        && !AGENT_MODELS
            .iter()
            .any(|known| known.agent == agent && known.model.eq_ignore_ascii_case(model))
}

#[derive(Debug, Clone, Deserialize)]
pub struct PricingResponse {
    pub models: Vec<PricingFileModel>,
}

pub fn load_pricing_overrides() -> Result<Vec<PricingFileModel>> {
    let path = crate::paths::pricing_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = std::fs::read_to_string(path)?;
    let response: PricingResponse = serde_json::from_str(&contents)?;
    Ok(response.models)
}

pub fn merged_agent_models() -> Result<Vec<ResolvedAgentModel>> {
    let mut merged = Vec::with_capacity(AGENT_MODELS.len());
    let mut indexes = HashMap::new();
    for model in AGENT_MODELS {
        indexes.insert((model.agent, model.model.to_lowercase()), merged.len());
        merged.push(ResolvedAgentModel::from(model));
    }
    for model in discovered_agy_models() {
        indexes.insert((model.agent, model.model.to_lowercase()), merged.len());
        merged.push(model);
    }
    for model in load_pricing_overrides()? {
        let Some(agent) = AgentKind::parse_str(&model.agent) else {
            continue;
        };
        let key = (agent, model.model.to_lowercase());
        if let Some(index) = indexes.get(&key).copied() {
            merged[index].apply_override(model);
        } else {
            indexes.insert(key, merged.len());
            merged.push(ResolvedAgentModel::from_override(agent, model));
        }
    }
    Ok(merged)
}
