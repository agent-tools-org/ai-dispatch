// aid CLI argument structs, part C.
// Exports clap Args types for top-level commands from worktree through web.

use crate::cli::{FindingCommands, MemoryCommands, StoreCommands};
use crate::cli_actions::{ProjectAction, TeamAction, ToolAction, WorktreeAction};
use clap::Args;

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid worktree create feat/my-feature
  aid worktree create fix/bug --base develop
  aid worktree list
  aid worktree prune
  aid worktree remove feat/my-feature"#)]
pub struct WorktreeArgs {
    #[command(subcommand)]
    pub action: WorktreeAction,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid store browse
  aid store install sunoj/aider
  aid store show sunoj/aider"#)]
pub struct StoreArgs {
    #[command(subcommand)]
    pub action: StoreCommands,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid team list
  aid team show dev
  aid team create dev"#)]
pub struct TeamArgs {
    #[command(subcommand)]
    pub action: TeamAction,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid tool list
  aid tool show lint-check
  aid tool add lint-check
  aid tool add scanner --team dev
  aid tool test lint-check file.ts"#)]
pub struct ToolArgs {
    #[command(subcommand)]
    pub action: ToolAction,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid project init
  aid project show
  aid project state"#)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid memory add discovery "The auth module uses bcrypt not argon2"
  aid memory add convention "Use anyhow::Result in CLI handlers" --tier critical
  aid memory list --type convention
  aid memory search "auth"
  aid memory forget m-a3f1"#)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub action: MemoryCommands,
}

#[derive(Args)]
pub struct FindingArgs {
    #[command(subcommand)]
    pub action: FindingCommands,
}

#[derive(Args)]
pub struct BroadcastArgs {
    pub group: String,
    pub message: String,
}

#[derive(Args)]
pub struct UpgradeArgs {
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct InternalRunTaskArgs {
    pub task_id: String,
}

#[derive(Args)]
pub struct TreeArgs {
    pub task_id: String,
}

#[derive(Args)]
pub struct OutputArgs {
    pub task_id: String,
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub brief: bool,
}

#[cfg(feature = "web")]
#[derive(Args)]
pub struct WebArgs {
    #[arg(long, default_value = "8080")]
    pub port: u16,
}
