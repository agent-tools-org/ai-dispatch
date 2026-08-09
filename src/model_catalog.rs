// Shared agent model catalog: per-agent model/pricing data and catalog queries.
// Exports: AGENT_PROFILES, AGENT_MODELS, AgentModel, PricingFileModel, ResolvedAgentModel,
//          models_for_agent(), model_for_task_budget(), budget_model(), load_pricing_overrides()
// Deps: crate::types::AgentKind, crate::paths, serde, model_catalog_data

use anyhow::Result;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::types::{AgentKind, TaskBudget};

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

/// Preferred catalog tiers for a declared budget, excluding unpriced `unknown`.
fn budget_preferred_tiers(budget: TaskBudget) -> &'static [&'static str] {
    match budget {
        TaskBudget::Free => &["free"],
        TaskBudget::Cheap => &["cheap", "free"],
        TaskBudget::Standard => &["standard", "cheap", "free"],
        TaskBudget::Premium => &["premium", "standard", "cheap", "free"],
    }
}

/// True when `model` sits on a preferred (priced/known) tier for `budget`.
pub fn model_on_budget_preference(kind: AgentKind, budget: TaskBudget, model: &str) -> bool {
    AGENT_MODELS
        .iter()
        .find(|entry| entry.agent == kind && entry.model == model)
        .is_some_and(|entry| budget_preferred_tiers(budget).contains(&entry.tier))
}

fn total_price(model: &AgentModel) -> f64 {
    model.input_per_m + model.output_per_m
}

/// Free/Cheap: lowest price (capability ties). Standard/Premium: highest capability.
fn better_budget_candidate(budget: TaskBudget, left: &AgentModel, right: &AgentModel) -> Ordering {
    match budget {
        TaskBudget::Free | TaskBudget::Cheap => total_price(left)
            .partial_cmp(&total_price(right))
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                right
                    .capability
                    .partial_cmp(&left.capability)
                    .unwrap_or(Ordering::Equal)
            }),
        TaskBudget::Standard | TaskBudget::Premium => right
            .capability
            .partial_cmp(&left.capability)
            .unwrap_or(Ordering::Equal),
    }
}

fn pick_in_tier(kind: AgentKind, budget: TaskBudget, tier: &str) -> Option<&'static AgentModel> {
    AGENT_MODELS
        .iter()
        .filter(|m| m.agent == kind && m.tier == tier)
        .min_by(|a, b| better_budget_candidate(budget, a, b))
}

/// Free/Cheap pool preferred tiers by lowest price; Standard/Premium walk tiers.
/// `unknown` is always last resort.
pub fn model_for_task_budget(kind: AgentKind, budget: TaskBudget) -> Option<&'static str> {
    let preferred = budget_preferred_tiers(budget);
    match budget {
        TaskBudget::Free | TaskBudget::Cheap => AGENT_MODELS
            .iter()
            .filter(|m| m.agent == kind && preferred.contains(&m.tier))
            .min_by(|a, b| better_budget_candidate(budget, a, b))
            .or_else(|| pick_in_tier(kind, budget, "unknown"))
            .map(|m| m.model),
        TaskBudget::Standard | TaskBudget::Premium => preferred
            .iter()
            .copied()
            .chain(std::iter::once("unknown"))
            .find_map(|tier| pick_in_tier(kind, budget, tier))
            .map(|m| m.model),
    }
}

/// Budget-mode / smart-route model: same rule as `model_for_task_budget(..., Cheap)`.
pub fn budget_model(agent: &AgentKind) -> Option<&'static str> {
    if *agent == AgentKind::Qwen {
        let models = models_for_agent(agent);
        if models.is_empty() {
            return None;
        }
        return get_qwen_selected_model()
            .and_then(|selected| {
                models
                    .iter()
                    .find(|model| model.model == selected)
                    .map(|model| model.model)
            })
            .or_else(|| models.first().map(|model| model.model));
    }
    model_for_task_budget(*agent, TaskBudget::Cheap)
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

#[cfg(test)]
#[path = "model_catalog_tests.rs"]
mod tests;
