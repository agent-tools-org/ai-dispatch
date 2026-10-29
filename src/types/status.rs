// Task lifecycle and event enums shared across store and UI layers.
// Exports: TaskStatus, PendingReason, VerifyStatus, EventKind.
// Deps: serde and std::fmt.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Waiting,
    Pending,
    Running,
    AwaitingInput,
    Stalled,
    Done,
    Merged,
    Failed,
    Skipped,
    Stopped,
}

impl TaskStatus {
    #[cfg(test)]
    pub const ALL: [Self; 10] = [
        Self::Waiting,
        Self::Pending,
        Self::Running,
        Self::AwaitingInput,
        Self::Stalled,
        Self::Done,
        Self::Merged,
        Self::Failed,
        Self::Skipped,
        Self::Stopped,
    ];

    pub fn parse_str(s: &str) -> Option<Self> {
        Self::from_str(s)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Waiting => "WAIT",
            Self::Pending => "PEND",
            Self::Running => "RUN",
            Self::AwaitingInput => "AWAIT",
            Self::Stalled => "STALL",
            Self::Done => "DONE",
            Self::Merged => "MERGED",
            Self::Failed => "FAIL",
            Self::Skipped => "SKIP",
            Self::Stopped => "STOP",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done | Self::Merged | Self::Failed | Self::Skipped | Self::Stopped
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        match self {
            Self::Waiting => matches!(next, Self::Pending | Self::Running | Self::Skipped | Self::Failed | Self::Stopped),
            Self::Pending => matches!(next, Self::Running | Self::Skipped | Self::Failed | Self::Stopped),
            Self::Running => matches!(next, Self::AwaitingInput | Self::Stalled | Self::Done | Self::Failed | Self::Stopped),
            Self::AwaitingInput => matches!(next, Self::Running | Self::Stalled | Self::Done | Self::Failed | Self::Stopped),
            Self::Stalled => matches!(next, Self::Running | Self::Done | Self::Failed | Self::Stopped),
            Self::Done => matches!(next, Self::Merged | Self::Failed),
            Self::Failed => matches!(next, Self::Merged),
            Self::Stopped => matches!(next, Self::Merged),
            Self::Merged | Self::Skipped => false,
        }
    }

}

super::string_values::string_values!(
    TaskStatus, as_str(&self), pub from_str {
        Waiting => "waiting",
        Pending => "pending",
        Running => "running",
        AwaitingInput => "awaiting_input",
        Stalled => "stalled",
        Done => "done",
        Merged => "merged",
        Failed => "failed",
        Skipped => "skipped",
        Stopped => "stopped",
    }
);

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingReason {
    Preparation,
    AgentStarting,
    RateLimited,
    WorkerCapacity,
    WaitTimeout,
    Unknown,
}

super::string_values::string_values!(
    PendingReason, as_str(&self), pub parse_str {
        Preparation => "preparation",
        AgentStarting => "agent_starting",
        RateLimited => "rate_limited",
        WorkerCapacity => "worker_capacity",
        WaitTimeout => "wait_timeout",
        Unknown => "unknown",
    }
);

impl fmt::Display for PendingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EventKind {
    Setup,
    ToolCall,
    Reasoning,
    Milestone,
    Build,
    Test,
    Commit,
    Completion,
    Error,
    NoOp,
    FileWrite,
    FileRead,
    WebSearch,
    Lint,
    Format,
}

impl EventKind {
    pub(crate) fn is_progress(self) -> bool {
        matches!(
            self,
            Self::Setup
                | Self::ToolCall
                | Self::Milestone
                | Self::Build
                | Self::Test
                | Self::Commit
                | Self::Completion
                | Self::Error
                | Self::FileWrite
                | Self::FileRead
                | Self::WebSearch
                | Self::Lint
                | Self::Format
        )
    }

    pub(crate) fn is_liveness(self) -> bool {
        !matches!(self, Self::NoOp)
    }

    /// Parse a stored event kind, warning once per unknown value before
    /// falling back to Reasoning.
    pub fn parse_or_warn(s: &str) -> Self {
        Self::parse_str(s).unwrap_or_else(|| {
            if note_unknown_event_kind(s) {
                crate::aid_warn!("[aid] Unknown event kind '{s}' in store; treating as reasoning");
            }
            Self::Reasoning
        })
    }
}

super::string_values::string_values!(
    EventKind, as_str(&self), pub parse_str {
        Setup => "setup",
        ToolCall => "tool_call",
        Reasoning => "reasoning",
        Milestone => "milestone",
        Build => "build",
        Test => "test",
        Commit => "commit",
        Completion => "completion",
        Error => "error",
        NoOp => "noop",
        FileWrite => "file_write",
        FileRead => "file_read",
        WebSearch => "web_search",
        Lint => "lint",
        Format => "format",
    }
);

/// Returns true the first time `raw` is seen, so the caller warns only once
/// per unknown kind per process.
pub(crate) fn note_unknown_event_kind(raw: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    match WARNED.lock() {
        Ok(mut guard) => guard
            .get_or_insert_with(HashSet::new)
            .insert(raw.to_string()),
        Err(_) => false,
    }
}
