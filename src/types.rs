// Core identifier and enum types for aid runtime data.
// Exports: TaskId, WorkgroupId, AgentKind, status enums, and task/memory re-exports.
// Deps: rand, serde, std::fmt.

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;
#[cfg(test)]
use std::{cell::RefCell, collections::VecDeque};

#[path = "types/agent.rs"]
mod agent;
#[path = "types/attribution.rs"]
mod attribution;
#[path = "types/provider.rs"]
mod provider;
#[path = "types/route.rs"]
mod route;
#[path = "types/delivery.rs"]
mod delivery;
#[path = "types/message.rs"]
mod message;
#[path = "types/status.rs"]
mod status;
#[path = "types/status_sets.rs"]
mod status_sets;
#[path = "types/task.rs"]
mod task;
#[path = "types/task_profile.rs"]
mod task_profile;
#[path = "types/memory.rs"]
mod memory;

pub use self::agent::AgentKind;
pub use self::attribution::{grade_observation, AttributionSource, ROUTER_ALIASES};
pub use self::provider::{model_family, provider_for_cli, MeteringShape, ProviderId};
pub use self::route::Route;
pub use self::delivery::DeliveryAssessment;
pub use self::message::{MessageDirection, MessageSource, TaskMessage};
pub use self::memory::{Memory, MemoryId, MemoryTier, MemoryType};
pub use self::status::{EventKind, PendingReason, TaskStatus, VerifyStatus};
pub use self::status_sets::{ACTIVE_EXECUTION_FAILURE_STATUSES, ACTIVE_TASK_STATUSES};
pub use self::task::{CompletionInfo, Finding, Task, TaskEvent, TaskFilter, Workgroup};
pub use self::task_profile::{
    DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskProfileDeclaration, TaskRigor,
    TaskUrgency,
};

/// Short hex ID prefixed with "t-", e.g. "t-a3f1b2c4"
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskId(pub String);

#[cfg(test)]
thread_local! {
    static TASK_ID_SEQUENCE: RefCell<VecDeque<String>> = RefCell::new(VecDeque::new());
}

impl TaskId {
    pub fn generate() -> Self {
        #[cfg(test)]
        if let Some(id) = TASK_ID_SEQUENCE.with(|ids| ids.borrow_mut().pop_front()) {
            return Self(id);
        }
        let val: u32 = rand::rng().random();
        Self(format!("t-{val:08x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn set_generate_sequence_for_tests(ids: &[&str]) {
        TASK_ID_SEQUENCE.with(|sequence| {
            *sequence.borrow_mut() = ids.iter().map(|id| id.to_string()).collect();
        });
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Short hex ID prefixed with "wg-", e.g. "wg-a3f1b2c4"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkgroupId(pub String);

impl WorkgroupId {
    pub fn generate() -> Self {
        let val: u32 = rand::rng().random();
        Self(format!("wg-{val:08x}"))
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
