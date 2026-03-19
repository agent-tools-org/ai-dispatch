// aid CLI argument structs, part B.
// Exports clap Args types for top-level commands from export through group.

use crate::cli::HookAction;
use crate::cli_actions::{ConfigAction, ContainerAction, GroupAction};
use clap::Args;

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid export t-1234
  aid export t-1234 --format json --output task.json"#)]
pub struct ExportArgs {
    pub task_id: String,
    #[arg(long, default_value = "md")]
    pub format: String,
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
}

#[derive(Args)]
pub struct MergeArgs {
    pub task_id: Option<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long)]
    pub approve: bool,
}

#[derive(Args)]
pub struct RespondArgs {
    pub task_id: String,
    pub input: Option<String>,
    #[arg(long, short)]
    pub file: Option<String>,
}

#[derive(Args)]
pub struct StopArgs {
    pub task_id: String,
    #[arg(long)]
    pub force: bool,
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
