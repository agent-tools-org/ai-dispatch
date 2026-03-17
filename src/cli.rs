// aid CLI definitions.
// Exports parser structs and subcommands; depends on clap derive.

use crate::cli_actions::{ConfigAction, GroupAction, ProjectAction, TeamAction, WorktreeAction};
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aid", version, about = "Multi-AI CLI team orchestrator")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args)]
pub(crate) struct RunExtrasArgs {
    /// Inject output from previous task(s) as context
    #[arg(long, num_args(1..))]
    pub(crate) context_from: Vec<String>,
    /// Methodology skills to inject
    #[arg(long, num_args(1..))]
    pub(crate) skill: Vec<String>,
    /// Prompt template to wrap around the task
    #[arg(long)]
    pub(crate) template: Option<String>,
    /// Command to run on task completion
    #[arg(long)]
    pub(crate) on_done: Option<String>,
    /// Agent cascade: comma-separated list of agents to try on failure (e.g. opencode,codex,cursor)
    #[arg(long, value_delimiter = ',')]
    pub(crate) cascade: Vec<String>,
    /// Hook specs to run for the dispatched task
    #[arg(long)]
    pub(crate) hook: Vec<String>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    #[command(after_help = r#"Examples:
  aid run codex "Add unit tests" --verify
  aid run gemini "Research topic" -o notes.md
  aid run codex "Refactor" -w feat/refactor --verify --retry 1 --bg

Hint: If passing file paths, use --context <path> not positional args"#)]
    /// Dispatch a task to an AI agent
    Run {
        /// Agent to use (auto, gemini, codex, opencode, cursor, kilo)
        agent: String,
        /// Prompt / task description
        prompt: String,
        /// Target repository path (defaults to current dir's git root)
        #[arg(long)]
        repo: Option<String>,
        /// Working directory for the agent
        #[arg(short, long)]
        dir: Option<String>,
        /// Output file path (for research tasks)
        #[arg(short, long)]
        output: Option<String>,
        /// Model override (for opencode)
        #[arg(short, long)]
        model: Option<String>,
        /// Prefer cheaper agents (avoid codex when quota is low)
        #[arg(long)]
        budget: bool,
        /// Run in a git worktree branch
        #[arg(short, long)]
        worktree: Option<String>,
        /// Boost preferred agents and inject team knowledge
        #[arg(long)]
        team: Option<String>,
        /// Reuse shared context from a workgroup
        #[arg(long, short = 'g')]
        group: Option<String>,
        /// Verify command (or auto-detect if flag given without value)
        #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
        verify: Option<String>,
        /// Judge agent to auto-review the task output (default gemini if omitted)
        #[arg(long, value_name = "AGENT", num_args = 0..=1, default_missing_value = "gemini")]
        judge: Option<String>,
        /// After completion, send output to a different agent for peer critique (scored 1-10)
        #[arg(long, value_name = "AGENT")]
        peer_review: Option<String>,
        /// Max retry attempts on failure
        #[arg(long, default_value = "0")]
        retry: u32,
        /// Context files to inject (can repeat or space-separate)
        #[arg(long, num_args(1..))]
        context: Vec<String>,
        /// Restrict agent to only read/modify files within scope
        #[arg(long, num_args(1..))]
        scope: Vec<String>,
        #[command(flatten)]
        run_extras: Box<RunExtrasArgs>,
        /// Disable automatic skill injection
        #[arg(long, conflicts_with = "skill")]
        no_skill: bool,
        /// Run in background
        #[arg(long)]
        bg: bool,
        /// Run in read-only mode (no file writes)
        #[arg(long)]
        read_only: bool,
        /// Dispatch to N budget-friendly agents and judge the best output
        #[arg(long, value_name = "N")]
        best_of: Option<usize>,
        /// Custom metric command for --best-of scoring (output must be a number)
        #[arg(long, value_name = "COMMAND", requires = "best_of")]
        metric: Option<String>,
        /// Link this task as a child of an existing task (thread composition)
        #[arg(long, value_name = "TASK_ID")]
        parent: Option<String>,
        /// Custom task ID (default: auto-generated t-xxxx)
        #[arg(long, value_name = "ID")]
        id: Option<String>,
    },
    #[command(after_help = r#"Examples:
  aid batch tasks.toml --parallel
  aid batch tasks.toml --parallel --max-concurrent 3

Batch TOML format:
  [defaults]
  dir = "."
  verify = "cargo check"
  agent = "codex"
  team = "dev"

  [[tasks]]
  name = "types"
  prompt = "Create shared types"
  worktree = "feat/types"

Note: --dir, --team, --verify are set in [defaults], not as CLI flags."#)]
    /// Dispatch tasks from a TOML batch file
    Batch {
        /// Path to the batch TOML file
        file: String,
        /// Dispatch tasks in parallel
        #[arg(long)]
        parallel: bool,
        /// Wait for dispatched tasks to finish
        #[arg(long)]
        wait: bool,
        /// Limit number of concurrent tasks
        #[arg(long)]
        max_concurrent: Option<usize>,
    },
    /// Benchmark a task across multiple agents
    Benchmark {
        /// Task prompt
        prompt: String,
        /// Comma-separated agents to compare
        #[arg(long)]
        agents: String,
        /// Working directory
        #[arg(short, long)]
        dir: Option<String>,
        /// Verify command
        #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
        verify: Option<String>,
    },
    #[command(after_help = r#"Examples:
  aid watch t-1234               # Live TUI for one task
  aid watch --quiet t-1234       # Block until done (for scripts)
  aid watch --quiet --group wg-a # Block until group finishes
  aid watch --tui                # Full dashboard TUI"#)]
    /// Live progress / blocking wait (--quiet)
    Watch {
        /// Watch specific task IDs (multiple allowed)
        task_ids: Vec<String>,
        /// Restrict to one workgroup in multi-task mode
        #[arg(long)]
        group: Option<String>,
        /// Interactive TUI mode
        #[arg(long)]
        tui: bool,
        /// Silent blocking wait (replaces `aid wait`)
        #[arg(long)]
        quiet: bool,
        /// Exit when task enters AwaitingInput, print task ID and prompt
        #[arg(long)]
        exit_on_await: bool,
    },
    /// List all tasks with status
    Board {
        /// Show only running tasks
        #[arg(long)]
        running: bool,
        /// Show only today's tasks
        #[arg(long)]
        today: bool,
        /// Show only tasks from the current caller session
        #[arg(long)]
        mine: bool,
        /// Show only tasks for one workgroup
        #[arg(long)]
        group: Option<String>,
        /// Stream task status updates to stdout
        #[arg(short, long)]
        stream: bool,
        /// Output as JSON (machine-parseable)
        #[arg(long)]
        json: bool,
    },
    /// Print the most recent completion notifications
    Completions,
    #[command(after_help = r#"Examples:
  aid agent list
  aid agent show aider
  aid agent add my-agent
  aid agent remove my-agent"#)]
    /// Manage custom agent definitions
    Agent {
        #[command(subcommand)]
        action: AgentCommands,
    },
    /// Clean up old tasks and orphaned worktrees
    Clean {
        /// Only clean tasks older than N days (default: 7)
        #[arg(long, default_value = "7")]
        older_than: u64,
        /// Also remove orphaned worktrees from /tmp/aid-wt-*
        #[arg(long)]
        worktrees: bool,
        /// Dry run — show what would be cleaned without deleting
        #[arg(long)]
        dry_run: bool,
    },
    #[command(after_help = r#"Examples:
  aid show t-1234              # Events timeline
  aid show t-1234 --diff       # Full worktree diff
  aid show t-1234 --output     # Task output
  aid show t-1234 --context    # Resolved prompt
  aid show t-1234 --explain    # AI explanation"#)]
    /// Inspect task artifacts (events, diff, output, explain)
    Show {
        /// Task ID to inspect
        task_id: String,
        /// Show events timeline (default behavior)
        #[arg(long)]
        events: bool,
        /// Show the full resolved prompt sent to the agent
        #[arg(long, help = "Show the full resolved prompt sent to the agent")]
        context: bool,
        /// Show full worktree diff
        #[arg(long)]
        diff: bool,
        /// Print output file
        #[arg(long)]
        output: bool,
        /// Dispatch AI explanation (creates child task)
        #[arg(long)]
        explain: bool,
        /// Print raw log file
        #[arg(long)]
        log: bool,
        /// Output as JSON (machine-parseable)
        #[arg(long)]
        json: bool,
        /// Agent for --explain (default: gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Model override for --explain
        #[arg(short, long)]
        model: Option<String>,
    },
    #[command(after_help = r#"Examples:
  aid export t-1234
  aid export t-1234 --format json --output task.json"#)]
    /// Export a task with full context
    Export {
        /// Task ID to export
        task_id: String,
        /// Output format (md or json)
        #[arg(long, default_value = "md")]
        format: String,
        /// Write export to a file
        #[arg(long)]
        output: Option<String>,
    },
    /// Show task-history usage and configured cost budgets
    Usage {
        /// Filter to current caller session
        #[arg(long)]
        session: bool,
        /// Show analytics for a specific agent (e.g. codex, gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Filter usage to a specific team's agents
        #[arg(long)]
        team: Option<String>,
        /// Time window to summarize (today, 7d, 30d, all)
        #[arg(long, default_value = "all")]
        period: String,
        /// Output raw JSON for automation
        #[arg(long)]
        json: bool,
    },
    /// Summarize workgroup results
    Summary {
        /// Workgroup ID (e.g. wg-abc1)
        group: String,
    },
    #[command(after_help = r#"Examples:
  aid retry t-1234 -f "Fix the compilation error in parser.rs"
  aid retry t-1234 -f "Use HashMap instead" --agent opencode"#)]
    /// Retry a failed task with feedback (optionally switch agent)
    Retry {
        /// Task ID to retry
        task_id: String,
        /// Feedback or correction for the agent
        #[arg(short, long)]
        feedback: String,
        /// Switch to a different agent (e.g. --agent opencode when codex is rate-limited)
        #[arg(long)]
        agent: Option<String>,
        /// Override working directory for this retry
        #[arg(long)]
        dir: Option<String>,
    },
    /// Mark task(s) as merged
    Merge {
        /// Task ID to mark as merged
        task_id: Option<String>,
        /// Mark all done tasks in a workgroup as merged
        #[arg(long)]
        group: Option<String>,
    },
    /// Send interactive input to a background task
    Respond {
        /// Task ID of the background task
        task_id: String,
        /// Response text (if omitted, reads from stdin)
        input: Option<String>,
        #[arg(long, short)]
        file: Option<String>,
    },
    /// Gracefully stop a running task (SIGTERM + wait)
    Stop {
        /// Task ID to stop
        task_id: String,
    },
    /// Force-kill a running task (SIGKILL)
    Kill {
        /// Task ID to kill
        task_id: String,
    },
    /// Inject guidance into a running task's PTY
    Steer {
        /// Task ID of the running task
        task_id: String,
        /// Message to inject (guidance, correction, etc.)
        message: String,
    },
    #[command(after_help = r#"Examples:
  aid ask "What is the latest Rust edition?"
  aid ask "Explain this error" --files src/main.rs -o explanation.md"#)]
    /// Research/explore via cheap AI CLIs
    Ask {
        /// Question or research prompt
        prompt: String,
        /// Agent to use (default: gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(short, long)]
        model: Option<String>,
        /// Files to include as context
        #[arg(long)]
        files: Vec<String>,
        /// Output file for the response
        #[arg(short, long)]
        output: Option<String>,
    },
    #[command(after_help = r#"Examples:
  aid query "What does gamma=0 mean in CryptoSwap?"
  aid query "Explain this" --auto
  aid query "Key insight" -g wg-abc1 --finding"#)]
    /// Fast LLM query via OpenRouter (no agent startup)
    Query {
        /// Question to ask
        prompt: String,
        /// Use auto-tier model (paid, better quality)
        #[arg(short, long)]
        auto: bool,
        /// Explicit model override
        #[arg(short, long)]
        model: Option<String>,
        /// Workgroup ID for context
        #[arg(short, long)]
        group: Option<String>,
        /// Save response as a workgroup finding
        #[arg(long)]
        finding: bool,
    },
    /// Start MCP server (stdio)
    Mcp,
    /// Manage agent configuration and detection
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage shared-context workgroups
    Group {
        #[command(subcommand)]
        action: GroupAction,
    },
    #[command(after_help = r#"Examples:
  aid worktree create feat/my-feature
  aid worktree create fix/bug --base develop
  aid worktree list
  aid worktree remove feat/my-feature"#)]
    /// Manage git worktrees for isolated task execution
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
    #[command(after_help = r#"Examples:
  aid store browse
  aid store install sunoj/aider
  aid store show sunoj/aider"#)]
    /// Browse and install agents from the community store
    Store {
        #[command(subcommand)]
        action: StoreCommands,
    },
    #[command(after_help = r#"Examples:
  aid team list
  aid team show dev
  aid team create dev"#)]
    /// Manage team definitions (agent groups for role-based selection)
    Team {
        #[command(subcommand)]
        action: TeamAction,
    },
    #[command(after_help = r#"Examples:
  aid project init
  aid project show"#)]
    /// Manage project configuration and knowledge
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    #[command(after_help = r#"Examples:
  aid memory add discovery "The auth module uses bcrypt not argon2"
  aid memory list --type convention
  aid memory search "auth"
  aid memory forget m-a3f1"#)]
    /// Manage agent shared memory (discoveries, conventions, lessons)
    Memory {
        #[command(subcommand)]
        action: MemoryCommands,
    },
    #[command(after_help = r#"Examples:
  aid finding add wg-abc1 "gamma can be zero in tricrypto"
  aid finding list wg-abc1"#)]
    /// Post or list workgroup findings
    Finding {
        #[command(subcommand)]
        action: FindingCommands,
    },
    /// Send a message to a workgroup's broadcast channel
    Broadcast {
        /// Workgroup ID
        group: String,
        /// Message to broadcast
        message: String,
    },
    /// Run autonomous experiment loop: edit → measure → keep/revert
    #[command(subcommand)]
    Experiment(ExperimentCommands),
    /// Upgrade aid to the latest version from crates.io
    Upgrade {
        /// Force upgrade even if tasks are running
        #[arg(long)]
        force: bool,
    },
    /// Initialize default skills and templates
    Init,
    /// Interactive setup wizard
    Setup,
    #[command(hide = true, name = "__run-task")]
    InternalRunTask { task_id: String },
    /// Show task retry tree
    Tree {
        /// Task ID
        task_id: String,
    },
    /// Print task output (shortcut for `show --output`)
    Output {
        /// Task ID
        task_id: String,
    },
}

#[derive(Subcommand)]
pub enum AgentCommands {
    /// List all agents (built-in + custom)
    List,
    /// Show agent details and configuration
    Show { name: String },
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
    Update {
        /// Memory ID (e.g. m-a3f1)
        id: String,
        /// New content
        content: String,
    },
    /// Delete a memory entry
    Forget {
        /// Memory ID (e.g. m-a3f1)
        id: String,
    },
    /// Show the version history for a memory chain
    History {
        /// Memory ID (e.g. m-a3f1)
        id: String,
    },
}

#[derive(Subcommand)]
pub enum FindingCommands {
    /// Post a finding to a workgroup
    Add {
        /// Workgroup ID
        group: String,
        /// Finding content
        content: String,
        /// Source task ID (optional)
        #[arg(long)]
        task: Option<String>,
    },
    /// List findings for a workgroup
    List {
        /// Workgroup ID
        group: String,
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

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, ExperimentCommands};
    use clap::Parser;

    #[test]
    fn run_best_of_flag_parses() {
        let cli =
            Cli::try_parse_from(["aid", "run", "auto", "add tests", "--best-of", "3"]).unwrap();
        match cli.command {
            Commands::Run { best_of, .. } => assert_eq!(best_of, Some(3)),
            _ => panic!("expected Run command"),
        }
    }

    #[test]
    fn run_parent_flag_parses() {
        let cli = Cli::try_parse_from(["aid", "run", "codex", "do stuff", "--parent", "t-abc123"])
            .unwrap();
        match cli.command {
            Commands::Run { parent, .. } => assert_eq!(parent, Some("t-abc123".to_string())),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn run_peer_review_flag_parses() {
        let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--peer-review", "gemini"])
            .unwrap();
        match cli.command {
            Commands::Run { peer_review, .. } => {
                assert_eq!(peer_review, Some("gemini".to_string()))
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn experiment_run_parses() {
        let cli = Cli::try_parse_from([
            "aid",
            "experiment",
            "run",
            "codex",
            "optimize perf",
            "--metric",
            "cargo bench 2>&1 | tail -1",
            "--direction",
            "min",
            "--max-runs",
            "10",
        ])
        .unwrap();
        match cli.command {
            Commands::Experiment(ExperimentCommands::Run {
                agent, max_runs, ..
            }) => {
                assert_eq!(agent, "codex");
                assert_eq!(max_runs, 10);
            }
            _ => panic!("expected Experiment Run"),
        }
    }
}
