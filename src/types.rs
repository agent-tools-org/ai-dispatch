// Domain types for aid tasks, workgroups, and event metadata.
// All types are serializable and keep IDs explicit rather than using raw strings.

use chrono::{DateTime, Local};
use rand::Rng;
use serde::Serialize;
use std::fmt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum AgentKind {
    Gemini,
    Codex,
    OpenCode,
    Cursor,
    Kilo,
}

impl AgentKind {
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            "cursor" => Some(Self::Cursor),
            "kilo" => Some(Self::Kilo),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Cursor => "cursor",
            Self::Kilo => "kilo",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskStatus {
    Pending,
    Running,
    AwaitingInput,
    Done,
    Merged,
    Failed,
    Skipped,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::AwaitingInput => "awaiting_input",
            Self::Done => "done",
            Self::Merged => "merged",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        Self::from_str(s)
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "awaiting_input" => Some(Self::AwaitingInput),
            "done" => Some(Self::Done),
            "merged" => Some(Self::Merged),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "PEND",
            Self::Running => "RUN",
            Self::AwaitingInput => "AWAIT",
            Self::Done => "DONE",
            Self::Merged => "MERGED",
            Self::Failed => "FAIL",
            Self::Skipped => "SKIP",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done | Self::Merged | Self::Failed | Self::Skipped
        )
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EventKind {
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

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: TaskId,
    pub agent: AgentKind,
    pub prompt: String,
    pub resolved_prompt: Option<String>,
    pub status: TaskStatus,
    pub parent_task_id: Option<String>,
    pub workgroup_id: Option<String>,
    pub caller_kind: Option<String>,
    pub caller_session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Workgroup {
    pub id: WorkgroupId,
    pub name: String,
    pub shared_context: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskEvent {
    pub task_id: TaskId,
    pub timestamp: DateTime<Local>,
    pub event_kind: EventKind,
    pub detail: String,
    pub metadata: Option<serde_json::Value>,
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
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
}
