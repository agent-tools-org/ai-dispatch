// aid CLI definitions.
// Exports parser structs and subcommands; depends on clap derive and cli helper modules.

pub(crate) mod command_args_a;
pub(crate) mod command_args_advise;
pub(crate) mod command_args_b;
pub(crate) mod command_args_c;
pub(crate) mod command_args_watch;
mod extras;
mod sub_enums;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod run_audit_flag_tests;
#[cfg(test)]
mod run_timeout_help_tests;
#[cfg(test)]
mod version_tests;
#[cfg(test)]
mod doctor_tests;
#[cfg(test)]
mod build_flag_tests;
#[cfg(test)]
mod retry_flag_tests;
#[cfg(test)]
mod respond_reply_flag_tests;
#[cfg(test)]
mod show_flag_tests;
#[cfg(test)]
mod watch_wait_flag_tests;

use clap::{Parser, Subcommand};

pub(crate) use extras::RunExtrasArgs;
pub use sub_enums::{
    AgentCommands, BatchAction, ByokCommands, ExperimentCommands, FindingCommands, HookAction,
    KgCommands, MemoryCommands, StoreCommands,
};

#[derive(Parser)]
#[command(
    name = "aid",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("AID_GIT_INFO"), ")"),
    about = "Multi-AI CLI team orchestrator with optional GitButler integration"
)]
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
    /// Inspect recent CLI errors, including requests rejected before task creation
    Errors(crate::command_diagnostics::ErrorsArgs),
    /// Show agent routing advice without dispatching
    Advise(command_args_advise::AdviseArgs),
    Batch(command_args_a::BatchArgs),
    Benchmark(command_args_a::BenchmarkArgs),
    Watch(command_args_watch::WatchArgs),
    Wait(command_args_watch::WaitArgs),
    Board(command_args_a::BoardArgs),
    /// Print recent notifications
    Notifications,
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
    /// Accept a completed task's delivered artifact as its principal.
    Accept(command_args_b::ArtifactDecisionArgs),
    /// Reject a completed task while preserving every artifact.
    Reject(command_args_b::ArtifactDecisionArgs),
    /// Delete accepted artifacts after recursive durability proof.
    Gc(command_args_b::ArtifactGcArgs),
    Respond(command_args_b::RespondArgs),
    Reply(command_args_b::ReplyArgs),
    Stop(command_args_b::StopArgs),
    #[command(hide = true)]
    Kill(command_args_b::KillArgs),
    Steer(command_args_b::SteerArgs),
    Unstick(command_args_b::UnstickArgs),
    Ask(command_args_b::AskArgs),
    Query(command_args_b::QueryArgs),
    Mcp,
    Hook(command_args_b::HookArgs),
    Config(command_args_b::ConfigArgs),
    Group(command_args_b::GroupArgs),
    Container(command_args_b::ContainerArgs),
    /// Run cargo build/check and parse/deduplicate JSON compiler errors
    Build(command_args_b::BuildArgs),
    /// Run cargo test with trusted guarantees (zero-match is an error)
    Test(command_args_b::TestArgs),
    Worktree(command_args_c::WorktreeArgs),
    Store(command_args_c::StoreArgs),
    Team(command_args_c::TeamArgs),
    Tool(command_args_c::ToolArgs),
    Doctor(command_args_c::DoctorArgs),
    /// Manage BYOK providers (custom OpenAI-compatible endpoints) via opencode
    Byok(command_args_c::ByokArgs),
    Credential(command_args_c::CredentialArgs),
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
