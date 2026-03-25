// aid CLI nested subcommands.
// Exports clap sub-enums reused by the top-level CLI; depends on clap derive.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum AgentCommands {
    /// List all agents (built-in + custom)
    List,
    /// Show agent details and configuration
    Show { name: String },
    /// Set or clear per-agent defaults
    Config {
        name: String,
        #[arg(long)]
        model: Option<String>,
        /// Default idle timeout in seconds (0 to clear)
        #[arg(long)]
        idle_timeout: Option<u64>,
    },
    /// Create a new custom agent definition
    Add { name: String },
    /// Remove a custom agent definition
    Remove { name: String },
    /// Fork a built-in or custom agent definition for local editing
    Fork {
        /// Name of the agent to fork (built-in or custom)
        name: String,
        /// Override the new agent name (defaults to `<name>-custom`)
        #[arg(long = "as")]
        new_name: Option<String>,
    },
    /// Show rate-limit / quota status for all agents
    Quota,
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Print session-start hook text for Claude Code
    SessionStart,
}

#[derive(Subcommand)]
pub enum StoreCommands {
    /// Browse available agents in the store
    Browse {
        /// Optional search query to filter agents
        query: Option<String>,
    },
    /// Install an agent from the store (publisher/name)
    Install { name: String },
    /// Show agent TOML from the store (publisher/name)
    Show { name: String },
    /// Check for updates to installed store packages
    Update {
        /// Apply available updates
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
pub enum MemoryCommands {
    /// Add a memory entry
    Add {
        /// Memory type: discovery, convention, lesson, fact
        #[arg(name = "TYPE")]
        memory_type: String,
        /// Content to remember
        content: String,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// List memories (project-scoped by default)
    List {
        /// Filter by type
        #[arg(long = "type")]
        memory_type: Option<String>,
        /// Show all memories across all projects
        #[arg(long)]
        all: bool,
        /// Include usage stats in the output
        #[arg(long)]
        stats: bool,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// Search memories by keyword
    Search {
        /// Search query
        query: String,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// Update a memory's content
    Update { id: String, content: String },
    /// Delete a memory entry
    Forget { id: String },
    /// Show the version history for a memory chain
    History { id: String },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum FindingCommands {
    /// Post a finding to a workgroup
    Add {
        /// Workgroup ID
        group: String,
        /// Finding content
        content: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        file: Option<String>,
        /// Source task ID (optional)
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        severity: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, name = "finding-file")]
        finding_file: Option<String>,
        #[arg(long)]
        lines: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        confidence: Option<String>,
    },
    /// List findings for a workgroup
    List {
        /// Workgroup ID
        group: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        count: bool,
        #[arg(long)]
        severity: Option<String>,
        #[arg(long)]
        verdict: Option<String>,
    },
    /// Show a single finding for a workgroup
    Get {
        /// Workgroup ID
        group: String,
        /// Finding ID
        finding_id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Update review metadata for a finding
    Update {
        /// Workgroup ID
        group: String,
        /// Finding ID
        finding_id: i64,
        #[arg(long)]
        verdict: Option<String>,
        #[arg(long)]
        score: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ExperimentCommands {
    /// Start an experiment loop
    Run {
        /// Agent to use for each iteration
        agent: String,
        /// Prompt describing what to optimize
        prompt: String,
        /// Command to measure the metric (output must be a number)
        #[arg(long)]
        metric: String,
        /// Optimization direction
        #[arg(long, default_value = "max")]
        direction: String,
        /// Correctness checks (must pass to keep changes)
        #[arg(long)]
        checks: Option<String>,
        /// Maximum number of experiment runs
        #[arg(long, default_value = "5")]
        max_runs: usize,
        /// Worktree branch for the experiment
        #[arg(long)]
        worktree: Option<String>,
        /// Verify command to run after each iteration
        #[arg(long)]
        verify: Option<String>,
    },
    /// Show experiment status and history
    Status {
        /// Working directory (where experiment.jsonl is)
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum BatchAction {
    /// Generate a template batch TOML file
    Init,
    /// Re-dispatch failed tasks from an existing batch workgroup
    Retry {
        /// Workgroup ID to retry failed tasks from
        group_id: String,
        /// Agent override for all retried tasks
        #[arg(long)]
        agent: Option<String>,
    },
}
