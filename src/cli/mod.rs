// aid CLI definitions.
// Exports parser structs and subcommands; depends on clap derive and cli helper modules.

pub(crate) mod command_args_a;
pub(crate) mod command_args_b;
pub(crate) mod command_args_c;
mod extras;
mod sub_enums;

#[cfg(test)]
mod tests;

use clap::{Parser, Subcommand};

pub(crate) use extras::RunExtrasArgs;
pub use sub_enums::{
    AgentCommands, BatchAction, ExperimentCommands, FindingCommands, HookAction, KgCommands,
    MemoryCommands, StoreCommands,
};

#[derive(Parser)]
#[command(name = "aid", version, about = "Multi-AI CLI team orchestrator")]
pub struct Cli {
    /// Suppress informational output (only errors/warnings shown). Also set via AID_QUIET=1.
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    Run(command_args_a::RunArgs),
    Batch(command_args_a::BatchArgs),
    Benchmark(command_args_a::BenchmarkArgs),
    Watch(command_args_a::WatchArgs),
    Board(command_args_a::BoardArgs),
    Completions,
    Changelog(command_args_a::ChangelogArgs),
    Agent(command_args_a::AgentArgs),
    Clean(command_args_a::CleanArgs),
    Show(command_args_a::ShowArgs),
    Export(command_args_b::ExportArgs),
    Usage(command_args_b::UsageArgs),
    Cost(command_args_b::CostArgs),
    Stats(command_args_b::StatsArgs),
    #[command(hide = true)]
    Summary(command_args_b::SummaryArgs),
    Retry(command_args_b::RetryArgs),
    Merge(command_args_b::MergeArgs),
    Respond(command_args_b::RespondArgs),
    Stop(command_args_b::StopArgs),
    #[command(hide = true)]
    Kill(command_args_b::KillArgs),
    Steer(command_args_b::SteerArgs),
    Ask(command_args_b::AskArgs),
    Query(command_args_b::QueryArgs),
    Mcp,
    Hook(command_args_b::HookArgs),
    Config(command_args_b::ConfigArgs),
    Group(command_args_b::GroupArgs),
    Container(command_args_b::ContainerArgs),
    Worktree(command_args_c::WorktreeArgs),
    Store(command_args_c::StoreArgs),
    Team(command_args_c::TeamArgs),
    Tool(command_args_c::ToolArgs),
    Project(command_args_c::ProjectArgs),
    Memory(command_args_c::MemoryArgs),
    /// Knowledge graph — temporal entity relationships
    Kg(command_args_c::KgArgs),
    #[command(hide = true)]
    Finding(command_args_c::FindingArgs),
    #[command(hide = true)]
    Broadcast(command_args_c::BroadcastArgs),
    #[command(subcommand)]
    Experiment(ExperimentCommands),
    Upgrade(command_args_c::UpgradeArgs),
    #[command(hide = true)]
    Init,
    Setup,
    #[command(hide = true, name = "__run-task")]
    InternalRunTask(command_args_c::InternalRunTaskArgs),
    Tree(command_args_c::TreeArgs),
    Output(command_args_c::OutputArgs),
    #[cfg(feature = "web")]
    #[command(name = "web")]
    Web(command_args_c::WebArgs),
}
