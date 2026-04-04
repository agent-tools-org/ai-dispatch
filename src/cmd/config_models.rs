// Agent config model data and pricing structures.
// Exports: AGENT_PROFILES, AGENT_MODELS, AgentModel, PricingFileModel, ResolvedAgentModel
// Deps: crate::types::AgentKind, serde

use serde::Deserialize;

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
        AgentKind::Codex,
        "Complex implementation, multi-file refactors, test suites",
        "$0.10-$8/M blended",
        "implement, create, build, refactor, test",
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
    AgentModel { agent: AgentKind::Codex, model: "gpt-5.4", input_per_m: 2.5, output_per_m: 15.0, tier: "premium", description: "Latest, best quality", capability: 9.4 },
    AgentModel { agent: AgentKind::Codex, model: "gpt-4.1", input_per_m: 2.0, output_per_m: 8.0, tier: "standard", description: "Reliable, good quality", capability: 8.7 },
    AgentModel { agent: AgentKind::Codex, model: "gpt-4.1-mini", input_per_m: 0.4, output_per_m: 1.6, tier: "cheap", description: "Balanced cost/quality", capability: 6.3 },
    AgentModel { agent: AgentKind::Codex, model: "gpt-4.1-nano", input_per_m: 0.1, output_per_m: 0.4, tier: "cheap", description: "Ultra-cheap, simple tasks", capability: 4.6 },
    AgentModel { agent: AgentKind::Gemini, model: "flash", input_per_m: 0.30, output_per_m: 2.50, tier: "cheap", description: "Fast, balanced (default)", capability: 7.3 },
    AgentModel { agent: AgentKind::Gemini, model: "pro", input_per_m: 1.25, output_per_m: 10.0, tier: "premium", description: "Complex reasoning", capability: 7.8 },
    AgentModel { agent: AgentKind::Gemini, model: "flash-lite", input_per_m: 0.0, output_per_m: 0.0, tier: "cheap", description: "Fastest, simple tasks", capability: 5.9 },
    AgentModel { agent: AgentKind::OpenCode, model: "glm-4.7", input_per_m: 0.38, output_per_m: 1.98, tier: "cheap", description: "Paid, good quality", capability: 6.5 },
    AgentModel { agent: AgentKind::OpenCode, model: "kimi-k2.5", input_per_m: 0.45, output_per_m: 2.20, tier: "cheap", description: "Paid, good quality", capability: 6.1 },
    AgentModel { agent: AgentKind::OpenCode, model: "mimo-v2-flash-free", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 4.3 },
    AgentModel { agent: AgentKind::OpenCode, model: "nemotron-3-super-free", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 4.1 },
    AgentModel { agent: AgentKind::OpenCode, model: "minimax-m2.5-free", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 4.1 },
    AgentModel { agent: AgentKind::Kilo, model: "default", input_per_m: 0.0, output_per_m: 0.0, tier: "free", description: "Free tier", capability: 3.8 },
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
