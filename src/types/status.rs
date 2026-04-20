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
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Pending => "pending",
            Self::Running => "running",
            Self::AwaitingInput => "awaiting_input",
            Self::Stalled => "stalled",
            Self::Done => "done",
            Self::Merged => "merged",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        Self::from_str(s)
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "waiting" => Some(Self::Waiting),
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "awaiting_input" => Some(Self::AwaitingInput),
            "stalled" => Some(Self::Stalled),
            "done" => Some(Self::Done),
            "merged" => Some(Self::Merged),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
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
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingReason {
    AgentStarting,
    RateLimited,
    WorkerCapacity,
    WaitTimeout,
    Unknown,
}

impl PendingReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentStarting => "agent_starting",
            Self::RateLimited => "rate_limited",
            Self::WorkerCapacity => "worker_capacity",
            Self::WaitTimeout => "wait_timeout",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "agent_starting" => Some(Self::AgentStarting),
            "rate_limited" => Some(Self::RateLimited),
            "worker_capacity" => Some(Self::WorkerCapacity),
            "wait_timeout" => Some(Self::WaitTimeout),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl fmt::Display for PendingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum VerifyStatus {
    Pending,
    Passed,
    Failed,
    Skipped,
}

impl VerifyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
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
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::ToolCall => "tool_call",
            Self::Reasoning => "reasoning",
            Self::Milestone => "milestone",
            Self::Build => "build",
            Self::Test => "test",
            Self::Commit => "commit",
            Self::Completion => "completion",
            Self::Error => "error",
            Self::NoOp => "noop",
            Self::FileWrite => "file_write",
            Self::FileRead => "file_read",
            Self::WebSearch => "web_search",
            Self::Lint => "lint",
            Self::Format => "format",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "setup" => Some(Self::Setup),
            "tool_call" => Some(Self::ToolCall),
            "reasoning" => Some(Self::Reasoning),
            "milestone" => Some(Self::Milestone),
            "build" => Some(Self::Build),
            "test" => Some(Self::Test),
            "commit" => Some(Self::Commit),
            "completion" => Some(Self::Completion),
            "error" => Some(Self::Error),
            "noop" => Some(Self::NoOp),
            "file_write" => Some(Self::FileWrite),
            "file_read" => Some(Self::FileRead),
            "web_search" => Some(Self::WebSearch),
            "lint" => Some(Self::Lint),
            "format" => Some(Self::Format),
            _ => None,
        }
    }
}
