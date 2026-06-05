// Core identifier and enum types for aid runtime data.
// Exports: TaskId, WorkgroupId, AgentKind, status enums, and task/memory re-exports.
// Deps: rand, serde, std::fmt.

use rand::Rng;
use serde::Serialize;
use std::fmt;
#[cfg(test)]
use std::{cell::RefCell, collections::VecDeque};

#[path = "types/agent.rs"]
mod agent;
#[path = "types/delivery.rs"]
mod delivery;
#[path = "types/message.rs"]
mod message;
#[path = "types/status.rs"]
mod status;
#[path = "types/task.rs"]
mod task;
#[path = "types/memory.rs"]
mod memory;

pub use self::agent::AgentKind;
pub use self::delivery::DeliveryAssessment;
pub use self::message::{MessageDirection, MessageSource, TaskMessage};
pub use self::memory::{Memory, MemoryId, MemoryTier, MemoryType};
pub use self::status::{EventKind, PendingReason, TaskStatus, VerifyStatus};
pub use self::task::{CompletionInfo, Finding, Task, TaskEvent, TaskFilter, Workgroup};

/// Short hex ID prefixed with "t-", e.g. "t-a3f1b2c4"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
