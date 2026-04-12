// aid CLI argument structs, part A.
// Exports clap Args types for top-level commands from run through show.

use crate::cli::{AgentCommands, BatchAction, RunExtrasArgs};
use clap::Args;

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
    pub budget: bool,
    #[arg(short, long)]
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
    #[arg(long)]
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
    #[arg(long)]
    pub timeout: Option<u64>,
    #[arg(long, value_name = "SECS")]
    pub idle_timeout: Option<u64>,
    #[arg(long, help = "Run aic cross-audit on this task after completion (requires `aic` binary on PATH)")]
    pub audit: bool,
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
  budget = false                         # Budget/cheap mode

  [[tasks]]
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
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub max_concurrent: Option<usize>,
    #[arg(short, long)]
    pub output: Option<String>,
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

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid watch t-1234               # Live TUI for one task
  aid watch --quiet t-1234       # Block until done (for scripts)
  aid watch --stream --group wg-a # JSONL events for orchestrators
  aid watch --quiet --group wg-a # Block until group finishes
  aid watch --tui                # Full dashboard TUI"#)]
pub struct WatchArgs {
    pub task_ids: Vec<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long)]
    pub tui: bool,
    #[arg(long)]
    pub quiet: bool,
    #[arg(long, conflicts_with_all = ["tui", "quiet", "exit_on_await"])]
    pub stream: bool,
    #[arg(long)]
    pub exit_on_await: bool,
    #[arg(long)]
    pub timeout: Option<u64>,
}

#[derive(Args, Default)]
pub struct BoardArgs {
    #[arg(long)]
    pub running: bool,
    #[arg(long)]
    pub today: bool,
    #[arg(long)]
    pub mine: bool,
    #[arg(long)]
    pub group: Option<String>,
    /// Maximum number of tasks to display (default: 50 without filters, unlimited with --group/--running/--today)
    #[arg(short, long)]
    pub limit: Option<usize>,
    /// Bypass anti-polling cooldown
    #[arg(long)]
    pub force: bool,
    #[arg(short, long)]
    pub stream: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ChangelogArgs {
    #[arg(long, conflicts_with_all = ["all","count"])]
    pub version: Option<String>,
    #[arg(long, conflicts_with = "version")]
    pub all: bool,
    #[arg(long, default_value = "5", conflicts_with = "version")]
    pub count: usize,
    #[arg(long)]
    pub git: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid agent list
  aid agent show aider
  aid agent add my-agent
  aid agent remove my-agent"#)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub action: AgentCommands,
}

#[derive(Args)]
pub struct CleanArgs {
    #[arg(long, default_value = "7")]
    pub older_than: u64,
    #[arg(long)]
    pub worktrees: bool,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid show t-1234              # Events timeline
  aid show t-1234 --diff       # Full worktree diff
  aid show t-1234 --output     # Task output (full)
  aid show t-1234 --transcript # Raw complete agent transcript
  aid show t-1234 --output --brief  # Task output (truncated)
  aid show t-1234 --context    # Resolved prompt
  aid show t-1234 --explain    # AI explanation"#)]
pub struct ShowArgs {
    pub task_id: String,
    #[arg(long)]
    pub events: bool,
    #[arg(long, help = "Show the full resolved prompt sent to the agent")]
    pub context: bool,
    #[arg(long)]
    pub diff: bool,
    #[arg(long, conflicts_with_all = ["diff", "output", "log"])]
    pub summary: bool,
    #[arg(long, requires = "diff")]
    pub file: Option<String>,
    #[arg(long)]
    pub output: bool,
    #[arg(long, conflicts_with_all = ["context", "diff", "summary", "output", "explain", "log", "json"])]
    pub transcript: bool,
    #[arg(long)]
    pub result: bool,
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub brief: bool,
    #[arg(long)]
    pub explain: bool,
    #[arg(long)]
    pub log: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(short, long)]
    pub model: Option<String>,
}
