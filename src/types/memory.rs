// Memory domain types for persisted lessons, facts, and conventions.
// Exports: MemoryId, MemoryType, MemoryTier, Memory.
// Deps: chrono, rand, serde, and std::fmt.

use chrono::{DateTime, Local};
use rand::Rng;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryId(pub String);

impl MemoryId {
    pub fn generate() -> Self {
        let val: u16 = rand::rng().random();
        Self(format!("m-{val:04x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MemoryType {
    Discovery,
    Convention,
    Lesson,
    Fact,
}

impl MemoryType {

    pub fn label(&self) -> &'static str {
        match self {
            Self::Discovery => "DISC",
            Self::Convention => "CONV",
            Self::Lesson => "LSSN",
            Self::Fact => "FACT",
        }
    }
}

super::string_values::string_values!(
    MemoryType, as_str(&self), pub parse_str {
        Discovery => "discovery",
        Convention => "convention",
        Lesson => "lesson",
        Fact => "fact",
    }
);

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MemoryTier {
    Identity,
    Critical,
    OnDemand,
    Deep,
}

super::string_values::string_values!(
    MemoryTier, as_str(&self), pub parse_str {
        Identity => "identity",
        Critical => "critical",
        OnDemand => "on_demand",
        Deep => "deep",
    }
);

impl fmt::Display for MemoryTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Memory {
    pub id: MemoryId,
    pub memory_type: MemoryType,
    pub tier: MemoryTier,
    pub content: String,
    pub source_task_id: Option<String>,
    pub agent: Option<String>,
    pub project_path: Option<String>,
    pub content_hash: String,
    pub created_at: DateTime<Local>,
    pub expires_at: Option<DateTime<Local>>,
    pub supersedes: Option<MemoryId>,
    pub version: i64,
    pub inject_count: i64,
    pub last_injected_at: Option<DateTime<Local>>,
    pub success_count: i64,
}
