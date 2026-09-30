// aid CLI arguments for project and worktree commands: project, worktree, clean, __run-task,
// experiment.
// Exports the matching clap Args structs and their subcommand enums; depends on clap derive.

use clap::{Args, Subcommand};

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid project init
  aid project show
  aid project state"#)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand)]
pub enum ProjectAction {
    /// Initialize project configuration in current repo
    Init,
    /// Show the detected project configuration
    Show,
    /// Show the current computed project state
    State,
    /// Sync project config to CLAUDE.md and global budget
    Sync,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid worktree create feat/my-feature
  aid worktree create fix/bug --base develop
  aid worktree list"#)]
pub struct WorktreeArgs {
    #[command(subcommand)]
    pub action: WorktreeAction,
}

#[derive(Subcommand)]
pub enum WorktreeAction {
    /// Create a worktree for a branch (prints path to stdout)
    Create {
        /// Branch name for the worktree
        branch: String,
        /// Base branch to fork from (default: HEAD)
        #[arg(long)]
        base: Option<String>,
        /// Repository path (defaults to current dir)
        #[arg(long)]
        repo: Option<String>,
    },
    /// List active aid-managed worktrees
    List {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Show only worktrees with live task locks
        #[arg(long)]
        active: bool,
        /// Repository path (defaults to current dir)
        #[arg(long)]
        repo: Option<String>,
    },
}

#[derive(Args)]
pub struct CleanArgs {
    #[arg(long, value_name = "DAYS", default_value = "7", help = "Remove records older than this many days")]
    pub older_than: u64,
    #[arg(long)]
    pub worktrees: bool,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct InternalRunTaskArgs {
    pub task_id: String,
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
