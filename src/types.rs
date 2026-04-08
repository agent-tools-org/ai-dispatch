// Domain types for aid tasks, workgroups, and event metadata.
// All types are serializable and keep IDs explicit rather than using raw strings.

use chrono::{DateTime, Local};
use rand::Rng;
use serde::{Deserialize, Serialize};
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
    Copilot,
    OpenCode,
    Cursor,
    Kilo,
    Codebuff,
    Droid,
    Oz,
    Claude,
    Custom,
}

impl AgentKind {
    pub const ALL_BUILTIN: &'static [Self] = &[
        Self::Gemini,
        Self::Codex,
        Self::Copilot,
        Self::OpenCode,
        Self::Cursor,
        Self::Kilo,
        Self::Codebuff,
        Self::Droid,
        Self::Oz,
        Self::Claude,
    ];

    pub const ALL: &'static [Self] = &[
        Self::Gemini,
        Self::Codex,
        Self::Copilot,
        Self::OpenCode,
        Self::Cursor,
        Self::Kilo,
        Self::Codebuff,
        Self::Droid,
        Self::Oz,
        Self::Claude,
        Self::Custom,
    ];

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "codex" => Some(Self::Codex),
            "copilot" => Some(Self::Copilot),
            "opencode" => Some(Self::OpenCode),
            "cursor" => Some(Self::Cursor),
            "kilo" => Some(Self::Kilo),
            "codebuff" => Some(Self::Codebuff),
            "droid" => Some(Self::Droid),
            "oz" => Some(Self::Oz),
            "claude" => Some(Self::Claude),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::OpenCode => "opencode",
            Self::Cursor => "cursor",
            Self::Kilo => "kilo",
            Self::Codebuff => "codebuff",
            Self::Droid => "droid",
            Self::Oz => "oz",
            Self::Claude => "claude",
            Self::Custom => "custom",
        }
    }

    pub fn sandboxed_fs(&self) -> bool {
        matches!(self, Self::OpenCode)
    }

    pub fn profile(
        &self,
    ) -> Option<(&'static str, &'static str, &'static str, &'static str, bool, &'static str)> {
        match self {
            Self::Gemini => Some((
                "gemini",
                "Research, coding, web search, file editing",
                "$0.10-$10/M blended",
                "research, explain, implement, create, analyze, build",
                true,
                "api",
            )),
            Self::Codex => Some((
                "codex",
                "Complex implementation, multi-file refactors",
                "$0.10-$8/M blended",
                "implement, create, build, refactor, test",
                true,
                "local",
            )),
            Self::Copilot => Some((
                "copilot",
                "General coding, repo navigation, tool-assisted implementation",
                "subscription",
                "implement, build, refactor, test, explain, debug",
                true,
                "api",
            )),
            Self::OpenCode => Some((
                "opencode",
                "Simple edits, renames, type annotations",
                "free-$2/M blended",
                "rename, change, update, fix typo, add type",
                true,
                "api",
            )),
            Self::Cursor => Some((
                "cursor",
                "General coding, strong model selection, frontend",
                "$20/mo subscription",
                "implement, create, build, refactor, ui, frontend, css",
                true,
                "api",
            )),
            Self::Kilo => Some((
                "kilo",
                "Simple edits (free tier)",
                "free",
                "rename, change, update, fix typo, add type",
                true,
                "api",
            )),
            Self::Codebuff => Some((
                "aid-codebuff",
                "Complex implementation, frontend",
                "SDK-managed",
                "complex coding, frontend",
                true,
                "local",
            )),
            Self::Droid => Some((
                "droid",
                "Complex implementation, multi-agent orchestration",
                "BYOK (API key)",
                "implement, create, build, refactor, test, orchestrate",
                true,
                "api",
            )),
            Self::Oz => Some((
                "oz",
                "Complex implementation, multi-file refactors",
                "Warp subscription",
                "implement, create, build, refactor, test",
                true,
                "local",
            )),
            Self::Claude => Some((
                "claude",
                "General coding, review, refactoring, research",
                "$1-$75/M blended",
                "implement, review, refactor, explain, research, test",
                true,
                "api",
            )),
            Self::Custom => None,
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
    Waiting,
    Pending,
    Running,
    AwaitingInput,
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
    EmptyDiff,
    Skipped,
}

impl VerifyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::EmptyDiff => "empty_diff",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "empty_diff" => Some(Self::EmptyDiff),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
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
    pub custom_agent_name: Option<String>,
    pub prompt: String,
    pub resolved_prompt: Option<String>,
    pub category: Option<String>,
    pub status: TaskStatus,
    pub parent_task_id: Option<String>,
    pub workgroup_id: Option<String>,
    pub caller_kind: Option<String>,
    pub caller_session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub start_sha: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub verify: Option<String>,
    pub verify_status: VerifyStatus,
    pub pending_reason: Option<String>,
    pub read_only: bool,
    pub budget: bool,
}

impl Task {
    /// Display name for the agent — uses custom_agent_name for custom agents.
    pub fn agent_display_name(&self) -> &str {
        if self.agent == AgentKind::Custom {
            self.custom_agent_name.as_deref().unwrap_or("custom")
        } else {
            self.agent.as_str()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Workgroup {
    pub id: WorkgroupId,
    pub name: String,
    pub shared_context: String,
    pub created_by: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: i64,
    pub workgroup_id: String,
    pub content: String,
    pub source_task_id: Option<String>,
    pub severity: Option<String>,
    pub title: Option<String>,
    pub file: Option<String>,
    pub lines: Option<String>,
    pub category: Option<String>,
    pub confidence: Option<String>,
    pub verdict: Option<String>,
    pub score: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: Option<DateTime<Local>>,
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
    Active,
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
    pub exit_code: Option<i32>,
}

/// Unique ID for a memory entry, prefixed with "m-"
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
    Discovery,  // Bug patterns, API behaviors, gotchas
    Convention, // Code style, naming, architecture decisions
    Lesson,     // What worked/failed in past tasks
    Fact,       // Version, config, endpoint facts
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn sample_task(agent: AgentKind, custom_agent_name: Option<&str>) -> Task {
        Task {
            id: TaskId("t-test".to_string()),
            agent,
            custom_agent_name: custom_agent_name.map(|name| name.to_string()),
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Pending,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn agent_display_name_returns_custom_name() {
        let task = sample_task(AgentKind::Custom, Some("my-tool"));
        assert_eq!(task.agent_display_name(), "my-tool");
    }

    #[test]
    fn agent_display_name_defaults_for_custom() {
        let task = sample_task(AgentKind::Custom, None);
        assert_eq!(task.agent_display_name(), "custom");
    }

    #[test]
    fn agent_display_name_for_built_in_agents() {
        let task = sample_task(AgentKind::Codex, None);
        assert_eq!(task.agent_display_name(), "codex");
    }

    #[test]
    fn memory_type_parse_str_roundtrip() {
        for memory_type in [
            MemoryType::Discovery,
            MemoryType::Convention,
            MemoryType::Lesson,
            MemoryType::Fact,
        ] {
            let s = memory_type.as_str();
            assert_eq!(MemoryType::parse_str(s), Some(memory_type));
        }
    }

    #[test]
    fn memory_tier_parse_str_roundtrip() {
        for memory_tier in [
            MemoryTier::Identity,
            MemoryTier::Critical,
            MemoryTier::OnDemand,
            MemoryTier::Deep,
        ] {
            let s = memory_tier.as_str();
            assert_eq!(MemoryTier::parse_str(s), Some(memory_tier));
        }
    }

    #[test]
    fn all_builtin_excludes_custom() {
        assert!(!AgentKind::ALL_BUILTIN.contains(&AgentKind::Custom));
    }

    #[test]
    fn all_includes_custom() {
        assert!(AgentKind::ALL.contains(&AgentKind::Custom));
    }

    #[test]
    fn all_builtin_matches_parse_str_coverage() {
        for kind in AgentKind::ALL_BUILTIN {
            assert_eq!(AgentKind::parse_str(kind.as_str()), Some(*kind));
        }
    }

    #[test]
    fn pending_reason_parse_str_roundtrip() {
        for reason in [
            PendingReason::AgentStarting,
            PendingReason::RateLimited,
            PendingReason::WorkerCapacity,
            PendingReason::Unknown,
        ] {
            let text = reason.as_str();
            assert_eq!(PendingReason::parse_str(text), Some(reason));
        }
    }

    #[test]
    fn profile_returns_some_for_all_builtin() {
        for kind in AgentKind::ALL_BUILTIN {
            assert!(kind.profile().is_some(), "{} should have a profile", kind.as_str());
        }
    }

    #[test]
    fn profile_returns_none_for_custom() {
        assert!(AgentKind::Custom.profile().is_none());
    }
}
