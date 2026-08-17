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
    pub capabilities: HashMap<String, i32>,
    pub models: ModelsJson,
    pub history: Option<HistoryJson>,
    pub load: LoadJson,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct QuotaJson {
    /// `"ok"` | `"partial"` (group hold — agent still dispatchable) | `"limited"` (agent hold)
    pub state: String,
    pub recovery_at: Option<String>,
    pub message: Option<String>,
    pub source: String, // "marker"
    /// Non-empty when state is `"partial"`. Each entry is one held model group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupHoldJson>,
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
