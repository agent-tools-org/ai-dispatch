// Domain types for aid: TaskId, AgentKind, TaskStatus, EventKind, Task, TaskEvent.
// All types are serializable and use strong typing over raw strings.

use chrono::{DateTime, Local};
use rand::Rng;
use std::fmt;

/// Short hex ID prefixed with "t-", e.g. "t-a3f1"
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Gemini,
    Codex,
    OpenCode,
}

impl AgentKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "PEND",
            Self::Running => "RUN",
            Self::Done => "DONE",
            Self::Failed => "FAIL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    ToolCall,
    Reasoning,
    Build,
    Test,
    Commit,
    Completion,
    Error,
    NoOp,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ToolCall => "tool_call",
            Self::Reasoning => "reasoning",
            Self::Build => "build",
            Self::Test => "test",
            Self::Commit => "commit",
            Self::Completion => "completion",
            Self::Error => "error",
            Self::NoOp => "noop",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "tool_call" => Some(Self::ToolCall),
            "reasoning" => Some(Self::Reasoning),
            "build" => Some(Self::Build),
            "test" => Some(Self::Test),
            "commit" => Some(Self::Commit),
            "completion" => Some(Self::Completion),
            "error" => Some(Self::Error),
            "noop" => Some(Self::NoOp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub agent: AgentKind,
    pub prompt: String,
    pub status: TaskStatus,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone)]
pub struct TaskEvent {
    pub task_id: TaskId,
    pub timestamp: DateTime<Local>,
    pub event_kind: EventKind,
    pub detail: String,
}

/// Filter for listing tasks
#[derive(Debug, Clone, Copy)]
pub enum TaskFilter {
    All,
    Running,
    Today,
}

/// Info extracted when an agent completes
#[derive(Debug, Clone)]
pub struct CompletionInfo {
    pub tokens: Option<i64>,
    pub status: TaskStatus,
}
