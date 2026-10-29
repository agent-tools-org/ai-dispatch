// Declared task-profile dimensions shared by CLI, batch, routing, and storage.
// Exports: TaskDifficulty, TaskBudget, TaskUrgency, TaskRigor, TaskEgress,
//   DeclaredTaskProfile.
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

super::string_values::string_values!(
    TaskDifficulty, label(self), pub(crate) parse_str {
        Trivial => "trivial",
        Simple => "simple",
        Moderate => "moderate",
        Complex => "complex",
    }
);

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

super::string_values::string_values!(
    TaskBudget, label(self), pub(crate) parse_str {
        Free => "free",
        Cheap => "cheap",
        Standard => "standard",
        Premium => "premium",
    }
);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskUrgency {
    Background,
    #[default]
    Normal,
    Urgent,
}

super::string_values::string_values!(
    TaskUrgency, label(self), pub(crate) parse_str {
        Background => "background",
        Normal => "normal",
        Urgent => "urgent",
    }
);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskRigor {
    Draft,
    #[default]
    Standard,
    Critical,
}

super::string_values::string_values!(
    TaskRigor, label(self), pub(crate) parse_str {
        Draft => "draft",
        Standard => "standard",
        Critical => "critical",
    }
);

/// Where task data may go. Decided by the provider endpoint, not the CLI.
/// Independent of [`TaskRigor`]: critical still forces verify+audit and does
/// not whitelist agents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TaskEgress {
    /// Any provider (default). No data-locality gate.
    #[default]
    Any,
    /// Only a provider whose established endpoint is loopback.
    Local,
    /// Only a provider whose established endpoint is loopback or private-network.
    PrivateNetwork,
}

impl TaskEgress {
    pub fn requires_local(self) -> bool {
        matches!(self, Self::Local)
    }

    pub fn requires_private_network(self) -> bool {
        matches!(self, Self::PrivateNetwork)
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
