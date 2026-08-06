// aid CLI argument structs, part B.
// Exports clap Args types for top-level commands from export through group.

use crate::cli::HookAction;
use crate::cli_actions::{ConfigAction, ContainerAction, GroupAction};
use clap::{Args, ValueEnum};

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
pub struct SummaryArgs {
    pub group: String,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid retry t-1234 -f "Fix the compilation error in parser.rs"
  aid retry t-1234 -f "Use HashMap instead" --agent opencode"#)]
pub struct RetryArgs {
    pub task_id: String,
    #[arg(short, long)]
    pub feedback: String,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub dir: Option<String>,
    #[arg(long)]
    pub reset: bool,
    #[arg(long, help = "Run in background (non-blocking)")]
    pub bg: bool,
}

#[derive(Args)]
pub struct MergeArgs {
    pub task_id: Option<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long)]
    pub approve: bool,
    #[arg(long)]
    pub check: bool,
    #[arg(long, help = "Allow merging a task in FAIL or STOPPED state (use when the task's code changes are good but verify failed).")]
    pub force: bool,
    #[arg(long, help = "Merge into this branch instead of current")]
    pub target: Option<String>,
    #[arg(long, help = "Apply group task branches as GitButler virtual branch lanes")]
    pub lanes: bool,
}

#[derive(Args)]
pub struct ArtifactDecisionArgs {
    /// Task whose delivered artifacts are being accepted or rejected.
    pub task_id: String,
}

#[derive(Args)]
pub struct ArtifactGcArgs {
    /// Accepted task whose worktree may be deleted after durability proof.
    #[arg(long)]
    pub task: String,
}

#[derive(Args)]
pub struct RespondArgs {
    pub task_id: String,
    pub input: Option<String>,
    #[arg(long, short = 'F')]
    pub file: Option<String>,
}

#[derive(Args)]
pub struct StopArgs {
    pub task_id: String,
    #[arg(long)]
    pub force: bool,
    /// Stop the entire retry tree containing this task — root + every retry
    /// descendant in a non-terminal state. The argument may be the root or
    /// any task in the chain; aid resolves to the root automatically.
    #[arg(long = "retry-tree")]
    pub retry_tree: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid reply t-1234 "Need status update"
  aid reply t-1234 -F reply.md
  aid reply t-1234 "continue" --async
  aid reply t-1234 "status?" --timeout 60"#)]
pub struct ReplyArgs {
    pub task_id: String,
    pub message: Option<String>,
    #[arg(long, short = 'F')]
    pub file: Option<String>,
    #[arg(long = "async")]
    pub async_mode: bool,
    #[arg(long = "timeout", value_name = "SECS", default_value = "30", help = "Wait this many seconds for an acknowledgement")]
    pub timeout_secs: u64,
}

#[derive(Args)]
pub struct KillArgs {
    pub task_id: String,
}

#[derive(Args)]
pub struct SteerArgs {
    pub task_id: String,
    pub message: String,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid unstick t-1234
  aid unstick t-1234 -m "Please summarize current blocker"
  aid unstick t-1234 --escalate"#)]
pub struct UnstickArgs {
    pub task_id: String,
    #[arg(long, short = 'm')]
    pub message: Option<String>,
    #[arg(long)]
    pub escalate: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid ask "What is the latest Rust edition?"
  aid ask "Explain this error" --files src/main.rs -o explanation.md"#)]
pub struct AskArgs {
    pub prompt: String,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(short, long)]
    pub model: Option<String>,
    #[arg(long)]
    pub files: Vec<String>,
    #[arg(short, long)]
    pub output: Option<String>,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid query "What does gamma=0 mean in CryptoSwap?"
  aid query "Explain this" --auto
  aid query "Key insight" -g wg-abc1 --finding"#)]
pub struct QueryArgs {
    pub prompt: String,
    #[arg(short, long)]
    pub auto: bool,
    #[arg(short, long)]
    pub model: Option<String>,
    #[arg(short, long)]
    pub group: Option<String>,
    #[arg(long)]
    pub finding: bool,
}

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub action: HookAction,
}

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Args)]
pub struct GroupArgs {
    #[command(subcommand)]
    pub action: GroupAction,
}

#[derive(Args)]
pub struct ContainerArgs {
    #[command(subcommand)]
    pub action: ContainerAction,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid build
  aid build check
  aid build clippy -- --all-targets

For trusted test runs (zero-match is an error, named executed tests), use `aid test`."#)]
pub struct BuildArgs {
    /// Cargo verification command. Defaults to project verify config, then check.
    /// Project verify of `cargo test …` maps to check; use `aid test` for tests.
    #[arg(value_enum)]
    pub command: Option<BuildCommandArg>,
    /// Cargo package to verify.
    #[arg(short = 'p', long)]
    pub package: Option<String>,
    /// Include warning diagnostics instead of reporting only their count.
    #[arg(long)]
    pub warnings: bool,
    /// Extra cargo arguments appended after aid's generated arguments.
    #[arg(last = true, allow_hyphen_values = true)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildCommandArg {
    Check,
    Clippy,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid test
  aid test --bin aid
  aid test --bin aid paths::aid_dir -- --exact
  aid test --lib
  aid test --isolated --bin aid my_test

Guarantees (unlike raw `cargo test`):
  - A filter matching zero tests exits non-zero and names the filter
  - A run with no test targets never looks like a pass
  - The digest lists which tests ran
  - Failure output is compact (panics/assertions, not pass noise)"#)]
pub struct TestArgs {
    /// Cargo package (-p).
    #[arg(short = 'p', long)]
    pub package: Option<String>,
    /// Only run tests for this binary target.
    #[arg(long)]
    pub bin: Option<String>,
    /// Only run library unit tests (`cargo test --lib`).
    #[arg(long)]
    pub lib: bool,
    /// Integration test target name (`cargo test --test NAME`).
    #[arg(long = "test")]
    pub test_target: Option<String>,
    /// Libtest name filter (substring or exact with `-- --exact`).
    pub filter: Option<String>,
    /// Run cargo test with a temporary AID_HOME (does not read/write ~/.aid).
    #[arg(long)]
    pub isolated: bool,
    /// Include warning diagnostics instead of reporting only their count.
    #[arg(long)]
    pub warnings: bool,
    /// Extra args after `--` are passed to the test harness (e.g. --exact).
    #[arg(last = true, allow_hyphen_values = true)]
    pub extra_args: Vec<String>,
}
