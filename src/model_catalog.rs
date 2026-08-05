// Shared agent model catalog: per-agent model/pricing data and catalog queries.
// Exports: AGENT_PROFILES, AGENT_MODELS, AgentModel, PricingFileModel, ResolvedAgentModel,
//          models_for_agent(), budget_model(), load_pricing_overrides()
// Deps: crate::types::AgentKind, crate::paths, serde, model_catalog_data

use anyhow::Result;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::types::AgentKind;

#[path = "model_catalog_data.rs"]
mod model_catalog_data;
pub use model_catalog_data::{AGENT_MODELS, AGENT_PROFILES, AgentModel};

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
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub tier: String,
    pub description: String,
}

impl From<&AgentModel> for ResolvedAgentModel {
    fn from(model: &AgentModel) -> Self {
        Self {
            agent: model.agent,
            model: model.model.to_string(),
            input_per_m: model.input_per_m,
            output_per_m: model.output_per_m,
            tier: model.tier.to_string(),
            description: model.description.to_string(),
        }
    }
}

impl ResolvedAgentModel {
    pub fn from_override(agent: AgentKind, model: PricingFileModel) -> Self {
        let PricingFileModel {
            model,
            input_per_m,
            output_per_m,
            tier,
            description,
            updated,
            ..
        } = model;
        let _ = updated;
        Self {
            agent,
            model,
            input_per_m,
            output_per_m,
            tier,
            description,
        }
    }

    pub fn apply_override(&mut self, model: PricingFileModel) {
        let PricingFileModel {
            input_per_m,
            output_per_m,
            tier,
            description,
            updated,
            ..
        } = model;
        let _ = updated;
        self.input_per_m = input_per_m;
        self.output_per_m = output_per_m;
        self.tier = tier;
        self.description = description;
    }
}

static QWEN_MODELS_CACHE: OnceLock<Vec<AgentModel>> = OnceLock::new();

fn load_qwen_models() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return vec!["coder-model".to_string()];
    };
    let path = home.join(".qwen").join("settings.json");
    if !path.exists() {
        return vec!["coder-model".to_string()];
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec!["coder-model".to_string()],
    };
    let settings: serde_json::Value = match serde_json::from_str(&content) {
        Ok(s) => s,
        Err(_) => return vec!["coder-model".to_string()],
    };

    let mut models = std::collections::BTreeSet::new();
    let mut has_providers = false;
    if let Some(providers) = settings.get("modelProviders") {
        if let Some(openai) = providers.get("openai").and_then(|v| v.as_array()) {
            for item in openai {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    models.insert(id.to_string());
                    has_providers = true;
                }
            }
        }
    }

    let selected_model = settings
        .get("model")
        .and_then(|model| model.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(ref name) = selected_model {
        models.insert(name.clone());
    } else if !has_providers {
        // keep empty → fallback below
    }

    if models.is_empty() {
        return vec!["coder-model".to_string()];
    }
    models.into_iter().collect()
}

pub fn get_qwen_selected_model() -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let path = home.join(".qwen").join("settings.json");
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let settings: serde_json::Value = serde_json::from_str(&content).ok()?;
    settings
        .get("model")
        .and_then(|model| model.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_qwen_models() -> &'static [AgentModel] {
    QWEN_MODELS_CACHE.get_or_init(|| {
        let models = load_qwen_models();
        models
            .into_iter()
            .map(|m| AgentModel {
                agent: AgentKind::Qwen,
                model: Box::leak(m.into_boxed_str()),
                input_per_m: 0.0,
                output_per_m: 0.0,
                tier: "free",
                description: "Default Qwen Code model",
                capability: 7.4,
            })
            .collect()
    })
}

pub fn models_for_agent(agent: &AgentKind) -> Vec<&'static AgentModel> {
    if *agent == AgentKind::Qwen {
        return get_qwen_models().iter().collect();
    }
    AGENT_MODELS
        .iter()
        .filter(|model| model.agent == *agent)
        .collect()
}

pub fn budget_model(agent: &AgentKind) -> Option<&'static str> {
    let models = models_for_agent(agent);
    if models.is_empty() {
        return None;
    }
    if *agent == AgentKind::Qwen {
        return get_qwen_selected_model()
            .and_then(|selected| {
                models
                    .iter()
                    .find(|model| model.model == selected)
                    .map(|model| model.model)
            })
            .or_else(|| models.first().map(|model| model.model));
    }
    let non_free: Vec<_> = models.iter().filter(|model| model.tier != "free").collect();
    if non_free.is_empty() {
        return models.first().map(|model| model.model);
    }
    non_free
        .iter()
        .min_by(|left, right| {
            let left_cost = left.input_per_m + left.output_per_m;
            let right_cost = right.input_per_m + right.output_per_m;
            left_cost.partial_cmp(&right_cost).unwrap_or(Ordering::Equal)
        })
        .map(|model| model.model)
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
    let contents = fs::read_to_string(path)?;
    let response: PricingResponse = serde_json::from_str(&contents)?;
    Ok(response.models)
}
