// aid CLI arguments for inspecting tasks.
// Exports Show, Board, Watch, Wait, Export, Tree, Usage, Cost, Stats and Changelog Args structs.
// Depends on clap derive.

use clap::{ArgGroup, Args};

#[derive(Args)]
#[command(group(
    ArgGroup::new("show_mode")
        .args(["events", "result", "json", "context", "explain", "summary", "diff", "output", "transcript", "log"])
        .multiple(false)
))]
#[command(after_help = r#"Examples:
  aid show t-1234              # Events timeline
  aid show t-1234 --diff       # What this task changed (start_sha..HEAD)
  aid show t-1234 --diff --branch # Every change on the task's branch
  aid show t-1234 --events     # Events only
  aid show t-1234 --output     # Task output (truncated)
  aid show t-1234 --output --full # Complete output
  aid show t-1234 --transcript # Raw complete agent transcript
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
    #[arg(long)]
    pub summary: bool,
    #[arg(long, requires = "diff")]
    pub file: Option<String>,
    #[arg(long, requires = "diff", help = "With --diff: every change on the task's branch, not just this task's own commits")]
    pub branch: bool,
    #[arg(long)]
    pub output: bool,
    #[arg(long)]
    pub transcript: bool,
    #[arg(long)]
    pub result: bool,
    #[arg(long)]
    pub full: bool,
    #[arg(long, conflicts_with = "full")]
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
    /// Show tasks from every project (default: current project only).
    /// Without this flag the board filters to the current project identity
    /// (or the explicit unattributed bucket when none is resolved).
    #[arg(long)]
    pub all: bool,
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
#[command(after_help = r#"Examples:
  aid watch t-1234                # Live task view
  aid watch --stream --group wg-a # JSONL events
  aid watch --tui                 # Full dashboard TUI"#)]
pub struct WatchArgs {
    pub task_ids: Vec<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long, conflicts_with_all = ["stream", "exit_on_await", "timeout"])]
    pub tui: bool,
    #[arg(long, conflicts_with_all = ["tui", "exit_on_await"])]
    pub stream: bool,
    #[arg(long, conflicts_with_all = ["tui", "stream"])]
    pub exit_on_await: bool,
    #[arg(long, value_name = "SECS", conflicts_with = "tui", help = "Stop watching after this many seconds")]
    pub timeout: Option<u64>,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid wait t-1234
  aid wait --group wg-a --timeout 60
  aid wait --exit-on-await t-1234"#)]
pub struct WaitArgs {
    pub task_ids: Vec<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long)]
    pub exit_on_await: bool,
    #[arg(long, value_name = "SECS", help = "Stop waiting after this many seconds")]
    pub timeout: Option<u64>,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid export t-1234
  aid export --sharegpt t-1234
  aid export t-1234 --format json --output task.json"#)]
pub struct ExportArgs {
    pub task_id: String,
    #[arg(long, default_value = "md")]
    pub format: String,
    #[arg(long)]
    pub sharegpt: bool,
    #[arg(long)]
    pub output: Option<String>,
}

#[derive(Args)]
pub struct TreeArgs {
    pub task_id: String,
}

#[derive(Args)]
pub struct UsageArgs {
    #[arg(long)]
    pub session: bool,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long, default_value = "all")]
    pub period: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid cost --group wg-abc1
  aid cost --summary
  aid cost --agent codex --period 30d"#)]
pub struct CostArgs {
    #[arg(long, conflicts_with_all = ["summary", "agent"])]
    pub group: Option<String>,
    #[arg(long, conflicts_with = "agent")]
    pub summary: bool,
    #[arg(long, conflicts_with = "group")]
    pub agent: Option<String>,
    #[arg(long, default_value = "7d")]
    pub period: String,
}

#[derive(Args)]
pub struct StatsArgs {
    #[arg(long, default_value = "7d")]
    pub window: String,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub insights: bool,
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
