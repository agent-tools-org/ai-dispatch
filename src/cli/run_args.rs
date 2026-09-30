// aid CLI arguments for dispatching work: run, batch and benchmark.
// Exports RunArgs, NO_HINT_FLAG, BatchArgs, BatchAction, BenchmarkArgs; depends on clap derive.

use crate::agent::classifier::TaskCategory;
use crate::cli::RunExtrasArgs;
use crate::types::{TaskBudget, TaskDifficulty, TaskEgress, TaskRigor, TaskUrgency};
use clap::{Args, Subcommand};

pub(crate) const NO_HINT_FLAG: &str = "no-hint";

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid run codex "Add unit tests" --verify
  aid run gemini "Research topic" -o notes.md
  aid run codex "Refactor" -w feat/refactor --verify --retry 1 --bg

Hint: If passing file paths, use --context <path> not positional args"#)]
pub struct RunArgs {
    pub agent: String,
    pub prompt: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub prompt_file: Option<String>,
    #[arg(long)]
    pub repo: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub repo_root: Option<String>,
    #[arg(short, long)]
    pub dir: Option<String>,
    #[arg(short, long)]
    pub output: Option<String>,
    #[arg(long, value_name = "FILE")]
    pub result_file: Option<String>,
    #[arg(short, long)]
    pub model: Option<String>,
    #[arg(long)]
    pub difficulty: Option<TaskDifficulty>,
    #[arg(long)]
    pub budget: Option<TaskBudget>,
    #[arg(long)]
    pub urgency: Option<TaskUrgency>,
    #[arg(long, help = "Proof level; critical requires enabled --verify and post-task --audit (or project defaults)")]
    pub rigor: Option<TaskRigor>,
    #[arg(long, default_value = "any", help = "Data egress: any (default), local (loopback only), or private-network (loopback or RFC1918/link-local)")]
    pub egress: TaskEgress,
    #[arg(long, help = "Task category; for a bug audit use debugging --read-only (--audit is a post-task check)")]
    pub kind: Option<TaskCategory>,
    #[arg(long = NO_HINT_FLAG)]
    pub no_hint: bool,
    #[arg(short, long, help = "Create/reuse a writable task branch; for a read-only checkout use --dir <path>")]
    pub worktree: Option<String>,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long, short = 'g')]
    pub group: Option<String>,
    #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
    pub verify: Option<String>,
    #[arg(long, value_name = "N")]
    pub iterate: Option<u32>,
    #[arg(long, value_name = "COMMAND", requires = "iterate")]
    pub eval: Option<String>,
    #[arg(long, value_name = "TEMPLATE", requires = "iterate")]
    pub eval_feedback_template: Option<String>,
    #[arg(long, value_name = "AGENT", num_args = 0..=1, default_missing_value = "gemini")]
    pub judge: Option<String>,
    #[arg(long, value_name = "AGENT")]
    pub peer_review: Option<String>,
    #[arg(long, default_value = "0")]
    pub retry: u32,
    #[arg(long, num_args(1..))]
    pub context: Vec<String>,
    #[arg(long, num_args(1..))]
    pub checklist: Vec<String>,
    #[arg(long, value_name = "FILE")]
    pub checklist_file: Option<String>,
    #[arg(long, num_args(1..))]
    pub scope: Vec<String>,
    #[command(flatten)]
    pub run_extras: Box<RunExtrasArgs>,
    #[arg(long, conflicts_with = "skill")]
    pub no_skill: bool,
    #[arg(long)]
    pub bg: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, help = "Read-only task; use --dir <checkout-path> to inspect an existing worktree")]
    pub read_only: bool,
    #[arg(long)]
    pub sandbox: bool,
    #[arg(long, value_name = "IMAGE", conflicts_with = "sandbox")]
    pub container: Option<String>,
    #[arg(long, value_name = "N")]
    pub best_of: Option<usize>,
    #[arg(long, value_name = "COMMAND", requires = "best_of")]
    pub metric: Option<String>,
    #[arg(long, value_name = "TASK_ID")]
    pub parent: Option<String>,
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,
    #[arg(long, value_name = "SECS", help = "Hard cap for the run in seconds; for minutes, multiply by 60")]
    pub timeout: Option<u64>,
    #[arg(long, value_name = "SECS", help = "Kill the agent after this many seconds without parsed activity")]
    pub idle_timeout: Option<u64>,
    #[arg(long, help = "Run aic cross-audit on this task after completion (requires `aic` binary on PATH)")]
    pub audit: bool,
    #[arg(long, conflicts_with = "audit", help = "Skip aic cross-audit for this task, even if enabled by project defaults")]
    pub no_audit: bool,
    #[arg(long)]
    pub no_link_deps: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid batch tasks.toml --parallel
  aid batch tasks.toml --analyze
  aid batch tasks.toml --parallel --max-concurrent 3
  aid batch init                         # Generate template TOML

Batch TOML format:
  [defaults]
  dir = "."                              # Working directory
  agent = "codex"                        # Default agent
  analyze = true                         # Warn about overlapping file edits
  team = "dev"                           # Team knowledge injection
  verify = "cargo check"                 # Auto-verify on completion
  fallback = "cursor"                    # Agent to try if primary fails
  model = "o3"                           # Model override
  context = ["src/types.rs"]             # Files to inject as context
  skills = ["implementer"]               # Methodology skills
  read_only = false                      # Read-only mode
  difficulty = "moderate"                # Task capability floor
  budget = "standard"                    # Model/cost tier
  urgency = "normal"                     # Rate-limit policy
  rigor = "standard"                     # Verification rigor
  egress = "any"                         # Data egress: any | local

  [[task]]
  name = "types"                         # Task name (for depends_on)
  agent = "codex"                        # Override default agent
  prompt = "Create shared types"         # Task prompt
  worktree = "feat/types"                # Git worktree branch
  fallback = "cursor"                    # Fallback agent on failure
  depends_on = ["other-task"]            # Run after named task(s)
  context = ["src/lib.rs"]               # Extra context files
  on_success = "deploy"                  # Trigger conditional task
  on_fail = "notify"                     # Trigger on failure

Note: --dir, --team, --verify are set in [defaults], not as CLI flags.
Run `aid batch init` to generate a full template with all fields."#)]
pub struct BatchArgs {
    #[command(subcommand)]
    pub action: Option<BatchAction>,
    pub file: Option<String>,
    #[arg(long = "var")]
    pub vars: Vec<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub repo_root: Option<String>,
    #[arg(long)]
    pub parallel: bool,
    #[arg(long)]
    pub analyze: bool,
    #[arg(long)]
    pub wait: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, help = "Skip interactive aid prompts and proceed without changing project config")]
    pub no_prompt: bool,
    #[arg(long, help = "Run non-interactively; skips interactive aid prompts")]
    pub yes: bool,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub max_concurrent: Option<usize>,
    #[arg(short, long)]
    pub output: Option<String>,
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
        /// Include tasks still stuck in WAIT status
        #[arg(long)]
        include_waiting: bool,
    },
}

#[derive(Args)]
pub struct BenchmarkArgs {
    pub prompt: String,
    #[arg(long)]
    pub agents: String,
    #[arg(short, long)]
    pub dir: Option<String>,
    #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
    pub verify: Option<String>,
}
