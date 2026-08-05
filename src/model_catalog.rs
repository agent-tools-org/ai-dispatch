// Shared agent model catalog: per-agent model/pricing data and catalog queries.
// Exports: AGENT_PROFILES, AGENT_MODELS, AgentModel, PricingFileModel, ResolvedAgentModel,
//          models_for_agent(), budget_model(), load_pricing_overrides()
// Deps: crate::types::AgentKind, crate::paths, serde

use anyhow::Result;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fs;

use crate::types::AgentKind;

pub const AGENT_PROFILES: &[(AgentKind, &str, &str, &str, bool)] = &[
    (
        AgentKind::Gemini,
        "Research, coding, web search, file editing",
        "$0.10-$10/M blended",
        "research, explain, implement, create, analyze, build",
        true,
    ),
    (
        AgentKind::Antigravity,
        "Research, coding, web search, file editing with Antigravity CLI",
        "free (Google One / Gemini Code Assist) or BYOK",
        "research, explain, implement, create, analyze, build",
        true,
    ),
    (
        AgentKind::Qwen,
        "Research, coding with Qwen3-Coder models",
        "free (OAuth) or Alibaba Cloud subscription",
        "implement, refactor, research, explain",
        true,
    ),
    (
        AgentKind::Codex,
        "Complex implementation, multi-file refactors, test suites",
        "$0.10-$8/M blended",
        "implement, create, build, refactor, test",
        true,
    ),
    (
        AgentKind::Copilot,
        "General coding, repo navigation, tool-assisted implementation",
        "subscription",
        "implement, build, refactor, test, explain, debug",
        true,
    ),
    (
        AgentKind::OpenCode,
        "Simple edits, renames, type annotations, quick fixes",
        "free-$2/M blended",
        "rename, change, update, fix typo, add type",
        true,
    ),
    (
        AgentKind::Kilo,
        "Simple edits, renames, type annotations (free tier)",
        "free",
        "rename, change, update, fix typo, add type",
        true,
    ),
    (
        AgentKind::MiMoCode,
        "Coding via Xiaomi MiMo Code CLI (opencode-family)",
        "free / key-store",
        "implement, change, update, refactor, add type",
        true,
    ),
    (
        AgentKind::Cursor,
        "General coding, strong model selection, frontend",
        "$20/mo subscription",
        "implement, create, build, refactor, ui, frontend, css",
        true,
    ),
    (
        AgentKind::Droid,
        "Complex implementation, multi-file refactors, debugging",
        "$3-$15/M blended",
        "implement, create, build, refactor, test, debug",
        true,
    ),
    (
        AgentKind::Claude,
        "General coding, review, refactoring, research",
        "$1-$75/M blended",
        "implement, review, refactor, explain, research, test",
        true,
    ),
];

pub struct AgentModel {
    pub agent: AgentKind,
    pub model: &'static str,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub tier: &'static str,
    pub description: &'static str,
    pub capability: f64,
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

pub const AGENT_MODELS: &[AgentModel] = &[
    AgentModel { agent: AgentKind::Codex, model: "gpt-5.5", input_per_m: 2.5, output_per_m: 15.0, tier: "premium", description: "Latest, best quality", capability: 9.6 },
    AgentModel { agent: AgentKind::Codex, model: "gpt-5.4", input_per_m: 2.5, output_per_m: 15.0, tier: "premium", description: "Reliable, good quality", capability: 9.3 },
    AgentModel { agent: AgentKind::Codex, model: "gpt-5.4-mini", input_per_m: 0.4, output_per_m: 1.6, tier: "cheap", description: "Balanced cost/quality", capability: 7.0 },
    AgentModel { agent: AgentKind::Gemini, model: "flash", input_per_m: 0.30, output_per_m: 2.50, tier: "cheap", description: "(version selected by gemini CLI)", capability: 8.0 },
    AgentModel { agent: AgentKind::Gemini, model: "pro", input_per_m: 1.25, output_per_m: 10.0, tier: "premium", description: "(version selected by gemini CLI)", capability: 9.0 },
    AgentModel { agent: AgentKind::Gemini, model: "flash-lite", input_per_m: 0.10, output_per_m: 0.40, tier: "cheap", description: "(version selected by gemini CLI)", capability: 6.5 },
    AgentModel { agent: AgentKind::Gemini, model: "gemini-3.1-pro-preview", input_per_m: 1.25, output_per_m: 10.0, tier: "premium", description: "Gemini 3.1 Pro (preview pricing)", capability: 9.0 },
    AgentModel { agent: AgentKind::Gemini, model: "gemini-3-flash-preview", input_per_m: 0.30, output_per_m: 2.50, tier: "cheap", description: "Gemini 3 Flash (preview pricing)", capability: 8.0 },
    AgentModel { agent: AgentKind::Gemini, model: "gemini-3-flash-lite-preview", input_per_m: 0.10, output_per_m: 0.40, tier: "cheap", description: "Gemini 3 Flash Lite (preview pricing)", capability: 6.5 },
    AgentModel { agent: AgentKind::Gemini, model: "gemini-2.5-flash", input_per_m: 0.30, output_per_m: 2.50, tier: "cheap", description: "Legacy 2.5 Flash (historical tasks)", capability: 7.3 },
    AgentModel { agent: AgentKind::Gemini, model: "gemini-2.5-pro", input_per_m: 1.25, output_per_m: 10.0, tier: "premium", description: "Legacy 2.5 Pro (historical tasks)", capability: 7.8 },
    AgentModel { agent: AgentKind::Qwen, model: "coder-model", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Default Qwen Code model", capability: 7.4 },
    AgentModel { agent: AgentKind::OpenCode, model: "opencode/glm-5.2", input_per_m: 0.38, output_per_m: 1.98, tier: "cheap", description: "Paid, good quality", capability: 6.8 },
    AgentModel { agent: AgentKind::OpenCode, model: "opencode/kimi-k2.6", input_per_m: 0.45, output_per_m: 2.20, tier: "cheap", description: "Paid, good quality", capability: 6.3 },
    AgentModel { agent: AgentKind::OpenCode, model: "opencode/deepseek-v4-flash-free", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 5.0 },
    AgentModel { agent: AgentKind::OpenCode, model: "opencode/nemotron-3-ultra-free", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 4.3 },
    AgentModel { agent: AgentKind::OpenCode, model: "opencode/mimo-v2.5-free", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 4.3 },
    AgentModel { agent: AgentKind::Kilo, model: "default", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 3.8 },
    AgentModel { agent: AgentKind::MiMoCode, model: "mimo/mimo-auto", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "MiMo Code (auto model)", capability: 3.8 },
    AgentModel { agent: AgentKind::Cursor, model: "composer-2", input_per_m: 0.50, output_per_m: 2.50, tier: "standard", description: "Composer 2, frontier coding model (default)", capability: 8.5 },
    AgentModel { agent: AgentKind::Cursor, model: "auto", input_per_m: 0.0, output_per_m: 0.0, tier: "cheap", description: "Auto routing, cheapest (recommended)", capability: 7.0 },
    AgentModel { agent: AgentKind::Cursor, model: "composer-1.5", input_per_m: 0.0, output_per_m: 0.0, tier: "standard", description: "Cursor proprietary, multi-file refactoring", capability: 8.0 },
    AgentModel { agent: AgentKind::Cursor, model: "opus-4.6-thinking", input_per_m: 0.0, output_per_m: 0.0, tier: "premium", description: "Strongest reasoning, premium pool", capability: 9.2 },
    AgentModel { agent: AgentKind::Cursor, model: "gpt-5.4-high", input_per_m: 0.0, output_per_m: 0.0, tier: "premium", description: "GPT-5.4 High, premium pool", capability: 9.0 },
    AgentModel { agent: AgentKind::Codebuff, model: "auto", input_per_m: 0.0, output_per_m: 0.0, tier: "standard", description: "SDK-managed pricing", capability: 6.8 },
    AgentModel { agent: AgentKind::Droid, model: "sonnet", input_per_m: 3.0, output_per_m: 15.0, tier: "standard", description: "Balanced cost/quality (default)", capability: 8.5 },
    AgentModel { agent: AgentKind::Droid, model: "opus", input_per_m: 15.0, output_per_m: 75.0, tier: "premium", description: "Strongest reasoning", capability: 9.5 },
    AgentModel { agent: AgentKind::Droid, model: "haiku", input_per_m: 0.25, output_per_m: 1.25, tier: "cheap", description: "Fast, simple tasks", capability: 5.8 },
    AgentModel { agent: AgentKind::Claude, model: "sonnet", input_per_m: 3.0, output_per_m: 15.0, tier: "standard", description: "Balanced coding and review", capability: 8.8 },
    AgentModel { agent: AgentKind::Claude, model: "opus", input_per_m: 15.0, output_per_m: 75.0, tier: "premium", description: "Best quality", capability: 9.4 },
    AgentModel { agent: AgentKind::Claude, model: "haiku", input_per_m: 0.8, output_per_m: 4.0, tier: "cheap", description: "Fastest, lower-cost option", capability: 6.2 },
];

use std::sync::OnceLock;
use std::path::PathBuf;

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

    // Try modelProviders.openai[].id
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

    // Try model.name
    let mut selected_model = None;
    if let Some(model) = settings.get("model") {
        if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
            selected_model = Some(name.to_string());
        }
    }

    if !has_providers {
        if let Some(ref name) = selected_model {
            models.insert(name.clone());
        }
    } else {
        if let Some(ref name) = selected_model {
            models.insert(name.clone());
        }
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
    settings.get("model")
        .and_then(|model| model.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_qwen_models() -> &'static [AgentModel] {
    QWEN_MODELS_CACHE.get_or_init(|| {
        let models = load_qwen_models();
        models.into_iter().map(|m| AgentModel {
            agent: AgentKind::Qwen,
            model: Box::leak(m.into_boxed_str()),
            input_per_m: 0.0,
            output_per_m: 0.0,
            tier: "free",
            description: "Default Qwen Code model",
            capability: 7.4,
        }).collect()
    })
}

pub fn models_for_agent(agent: &AgentKind) -> Vec<&'static AgentModel> {
    if *agent == AgentKind::Qwen {
        return get_qwen_models().iter().collect();
    }
    AGENT_MODELS.iter().filter(|model| model.agent == *agent).collect()
}


pub fn budget_model(agent: &AgentKind) -> Option<&'static str> {
    let models = models_for_agent(agent);
    if models.is_empty() {
        return None;
    }
    // Qwen's models come from the user's plan config and all carry the same
    // subscription price, so "cheapest" is meaningless and `models.first()` is a
    // byte-order accident — it picked MiniMax-M2.5 out of a 17-model plan and every
    // short prompt 403'd. The model the user selected is the only defensible answer.
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
