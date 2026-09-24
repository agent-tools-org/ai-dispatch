// Serializable response types for `aid agent list --json` and agent detail output.
// Exports agent, quota, model, history, and load JSON contracts.
// Deps: serde and HashMap.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AgentListJson {
    pub generated_at: String,
    pub agents: Vec<AgentJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AgentJson {
    pub name: String,
    pub kind: String, // "builtin" or "custom"
    pub installed: bool,
    pub disabled: bool,
    pub trust_tier: String,
    pub description: String,
    pub supports_session_resume: bool,
    /// The billing entity behind this CLI, and how it meters. `unknown` when
    /// aid has never observed the provider refuse and so cannot name it.
    pub provider: String,
    pub metering: String,
    pub quota: QuotaJson,
    /// `failed` (a run hit a not-signed-in refusal within the last hour, with
    /// `observed_at`) or `unknown` (no evidence). Never `ok`.
    pub auth: crate::auth_marker::AuthStatus,
    pub capabilities: HashMap<String, i32>,
    pub models: ModelsJson,
    pub history: Option<HistoryJson>,
    pub load: LoadJson,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct QuotaJson {
    /// `"ok"` (a successful probe observed it) | `"unknown"` (no evidence) | `"partial"`
    /// (group hold — agent still dispatchable) | `"limited"` (agent hold) | `"degraded"`
    pub state: String,
    pub recovery_at: Option<String>,
    pub message: Option<String>,
    pub source: String, // "probe" | "marker" | "none"
    /// Non-empty when state is `"partial"`. Each entry is one held model group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupHoldJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    #[serde(default)]
    pub stale: bool,
}

/// One held model-group entry inside a `partial` quota state.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GroupHoldJson {
    pub group: String,
    pub recovery_at: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ModelsJson {
    pub default: Option<String>,
    /// The resolver source of `default` (`sticky`, `custom_forced`, `cli_config`,
    /// `budget_route`); null when the default is unknown.
    pub default_source: Option<String>,
    pub budget: Option<String>,
    pub available: Vec<AvailableModelJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AvailableModelJson {
    pub model: String,
    pub tier: String,
    pub input_per_m: Option<f64>,
    pub output_per_m: Option<f64>,
    pub capability: Option<f64>,
    /// False when aid has no measured capability for this model.
    pub rated: bool,
    /// `"catalog"` | `"served"` (CLI reports it, no catalog row) | `"pricing_override"`.
    pub source: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HistoryJson {
    pub window_days: u32,
    pub tasks: u64,
    pub success_rate: f64,
    pub avg_duration_secs: Option<f64>,
    pub avg_cost_usd: Option<f64>,
    pub by_category: HashMap<String, CategoryHistoryJson>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CategoryHistoryJson {
    pub tasks: u64,
    pub success_rate: f64,
    pub avg_duration_secs: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LoadJson {
    pub running: u64,
}
