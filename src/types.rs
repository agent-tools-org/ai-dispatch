// Core identifier and enum types for aid runtime data.
// Exports: TaskId, WorkgroupId, AgentKind, status enums, and task/memory re-exports.
// Deps: rand, serde, std::fmt.

use rand::Rng;
use serde::Serialize;
use std::fmt;

#[path = "types/agent.rs"]
mod agent;
#[path = "types/delivery.rs"]
mod delivery;
#[path = "types/status.rs"]
mod status;
#[path = "types/task.rs"]
mod task;
#[path = "types/memory.rs"]
mod memory;

pub use self::agent::AgentKind;
pub use self::delivery::DeliveryAssessment;
pub use self::memory::{Memory, MemoryId, MemoryTier, MemoryType};
pub use self::status::{EventKind, PendingReason, TaskStatus, VerifyStatus};
pub use self::task::{CompletionInfo, Finding, Task, TaskEvent, TaskFilter, Workgroup};

/// Short hex ID prefixed with "t-", e.g. "t-a3f1"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskId(pub String);

impl TaskId {
    pub fn generate() -> Self {
        let val: u16 = rand::rng().random();
        Self(format!("t-{val:04x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Short hex ID prefixed with "wg-", e.g. "wg-a3f1"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkgroupId(pub String);

impl WorkgroupId {
    pub fn generate() -> Self {
        let val: u16 = rand::rng().random();
        Self(format!("wg-{val:04x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkgroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
#[path = "types/tests.rs"]
mod tests;
