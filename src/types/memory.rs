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
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Convention => "convention",
            Self::Lesson => "lesson",
            Self::Fact => "fact",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "discovery" => Some(Self::Discovery),
            "convention" => Some(Self::Convention),
            "lesson" => Some(Self::Lesson),
            "fact" => Some(Self::Fact),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Discovery => "DISC",
            Self::Convention => "CONV",
            Self::Lesson => "LSSN",
            Self::Fact => "FACT",
        }
    }
}

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

impl MemoryTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Critical => "critical",
            Self::OnDemand => "on_demand",
            Self::Deep => "deep",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "identity" => Some(Self::Identity),
            "critical" => Some(Self::Critical),
            "on_demand" => Some(Self::OnDemand),
            "deep" => Some(Self::Deep),
            _ => None,
        }
    }
}

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
