// Owned model-catalog records, including CLI-discovered models and pricing overrides.
// Exports resolved catalog queries without adding a second discovery cache.
// Deps: static model catalog, served-only rows (model_catalog_served), serde.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;

use super::model_catalog_served::{served_only_models, SERVED_PROBE_AGENTS};
use super::{static_models_for_agent, AgentModel, AGENT_MODELS};
use crate::types::AgentKind;

/// Where a catalog row came from. Only `Catalog` rows carry a rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOrigin {
    Catalog,
    Served,
    PricingOverride,
}

impl ModelOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Served => "served",
            Self::PricingOverride => "pricing_override",
        }
    }
}

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
    pub tier: String,
    pub description: String,
    pub capability: Option<f64>,
    pub origin: ModelOrigin,
}

impl From<&AgentModel> for ResolvedAgentModel {
    fn from(model: &AgentModel) -> Self {
        Self {
            agent: model.agent,
            model: model.model.to_string(),
            tier: model.tier.to_string(),
            description: model.description.to_string(),
            capability: Some(model.capability),
            origin: ModelOrigin::Catalog,
        }
    }
}

impl ResolvedAgentModel {
    pub fn from_override(agent: AgentKind, model: PricingFileModel) -> Self {
        let PricingFileModel { model, tier, description, .. } = model;
        Self {
            agent,
            model,
            tier,
            description,
            capability: None,
            origin: ModelOrigin::PricingOverride,
        }
    }
}

pub fn models_for_agent(agent: &AgentKind) -> Vec<ResolvedAgentModel> {
    let mut models: Vec<_> = static_models_for_agent(agent)
        .into_iter()
        .map(ResolvedAgentModel::from)
        .collect();
    models.extend(served_only_models(*agent));
    models
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
    let mut listed = HashSet::new();
    for model in AGENT_MODELS {
        listed.insert((model.agent, model.model.to_lowercase()));
        merged.push(ResolvedAgentModel::from(model));
    }
    for model in SERVED_PROBE_AGENTS.iter().flat_map(|agent| served_only_models(*agent)) {
        listed.insert((model.agent, model.model.to_lowercase()));
        merged.push(model);
    }
    for model in load_pricing_overrides()? {
        let Some(agent) = AgentKind::parse_str(&model.agent) else {
            continue;
        };
        // Rows already listed take the override price from cost::resolve_pricing.
        if listed.insert((agent, model.model.to_lowercase())) {
            merged.push(ResolvedAgentModel::from_override(agent, model));
        }
    }
    Ok(merged)
}
