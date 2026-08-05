// Declared task-profile dimensions shared by CLI, batch, routing, and storage.
// Exports: TaskDifficulty, TaskBudget, TaskUrgency, TaskRigor, DeclaredTaskProfile.
// Deps: clap value parsing and serde persistence/JSON.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskDifficulty {
    Trivial,
    Simple,
    #[default]
    Moderate,
    Complex,
}

impl TaskDifficulty {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trivial => "trivial",
            Self::Simple => "simple",
            Self::Moderate => "moderate",
            Self::Complex => "complex",
        }
    }

    pub(crate) fn parse_str(value: &str) -> Option<Self> {
        match value {
            "trivial" => Some(Self::Trivial),
            "simple" => Some(Self::Simple),
            "moderate" => Some(Self::Moderate),
            "complex" => Some(Self::Complex),
            _ => None,
        }
    }

    pub(crate) fn capability_floor(self) -> i32 {
        match self {
            Self::Trivial => 1,
            Self::Simple => 4,
            Self::Moderate => 6,
            Self::Complex => 8,
        }
    }

    /// Ordered capability ladder for nested-delegation ceilings.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Trivial => 0,
            Self::Simple => 1,
            Self::Moderate => 2,
            Self::Complex => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskBudget {
    Free,
    Cheap,
    #[default]
    Standard,
    Premium,
}

impl TaskBudget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Cheap => "cheap",
            Self::Standard => "standard",
            Self::Premium => "premium",
        }
    }

    pub(crate) fn parse_str(value: &str) -> Option<Self> {
        match value {
            "free" => Some(Self::Free),
            "cheap" => Some(Self::Cheap),
            "standard" => Some(Self::Standard),
            "premium" => Some(Self::Premium),
            _ => None,
        }
    }

    pub(crate) fn uses_budget_mode(self) -> bool {
        matches!(self, Self::Free | Self::Cheap)
    }

    /// Ordered spend ladder for nested-delegation ceilings.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Cheap => 1,
            Self::Standard => 2,
            Self::Premium => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskUrgency {
    Background,
    #[default]
    Normal,
    Urgent,
}

impl TaskUrgency {
    pub fn label(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Normal => "normal",
            Self::Urgent => "urgent",
        }
    }

    pub(crate) fn parse_str(value: &str) -> Option<Self> {
        match value {
            "background" => Some(Self::Background),
            "normal" => Some(Self::Normal),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskRigor {
    Draft,
    #[default]
    Standard,
    Critical,
}

impl TaskRigor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Standard => "standard",
            Self::Critical => "critical",
        }
    }

    pub(crate) fn parse_str(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "standard" => Some(Self::Standard),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredTaskProfile {
    pub difficulty: TaskDifficulty,
    pub budget: TaskBudget,
    pub urgency: TaskUrgency,
    pub rigor: TaskRigor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProfileDeclaration {
    pub difficulty: Option<TaskDifficulty>,
    pub budget: Option<TaskBudget>,
    pub urgency: Option<TaskUrgency>,
    pub rigor: Option<TaskRigor>,
}
