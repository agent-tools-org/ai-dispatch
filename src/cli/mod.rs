// aid CLI definitions.
// Exports parser structs and subcommands; depends on clap derive and cli helper modules.

pub(crate) mod admin_args;
pub(crate) mod advise_args;
pub(crate) mod agent_provider_args;
pub(crate) mod cargo_args;
pub(crate) mod classify_args;
mod extras;
pub(crate) mod group_args;
pub(crate) mod inspect_args;
pub(crate) mod knowledge_args;
pub(crate) mod project_args;
pub(crate) mod run_args;
pub(crate) mod task_control_args;

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
#[cfg(test)]
mod removed_path_tests;

use clap::{Parser, Subcommand};

pub(crate) use extras::RunExtrasArgs;
pub use admin_args::{ConfigAction, ContainerAction, HookAction, StoreCommands};
pub use agent_provider_args::{
    AgentCommands, ByokCommands, CredentialAction, TeamAction, ToolAction,
};
pub use group_args::{FindingCommands, GroupAction};
pub use knowledge_args::{KgCommands, MemoryCommands};
pub use project_args::{ExperimentCommands, ProjectAction, WorktreeAction};
pub use run_args::BatchAction;

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
    Run(run_args::RunArgs),
    /// Inspect recent CLI errors, including requests rejected before task creation
    Errors(crate::command_diagnostics::ErrorsArgs),
    /// Show agent routing advice without dispatching
    Advise(advise_args::AdviseArgs),
    Batch(run_args::BatchArgs),
    Benchmark(run_args::BenchmarkArgs),
    Watch(inspect_args::WatchArgs),
    Wait(inspect_args::WaitArgs),
    Board(inspect_args::BoardArgs),
    /// Print recent notifications
    Notifications,
    Changelog(inspect_args::ChangelogArgs),
    Agent(agent_provider_args::AgentArgs),
    Clean(project_args::CleanArgs),
    Show(inspect_args::ShowArgs),
    Export(inspect_args::ExportArgs),
    Usage(inspect_args::UsageArgs),
    Cost(inspect_args::CostArgs),
    Stats(inspect_args::StatsArgs),
    Retry(task_control_args::RetryArgs),
    Merge(task_control_args::MergeArgs),
    /// Accept a completed task's delivered artifact as its principal.
    Accept(task_control_args::ArtifactDecisionArgs),
    /// Reject a completed task while preserving every artifact.
    Reject(task_control_args::ArtifactDecisionArgs),
    /// Delete accepted artifacts after recursive durability proof.
    Gc(task_control_args::ArtifactGcArgs),
    Respond(task_control_args::RespondArgs),
    Reply(task_control_args::ReplyArgs),
    Stop(task_control_args::StopArgs),
    Steer(task_control_args::SteerArgs),
    Unstick(task_control_args::UnstickArgs),
    Ask(knowledge_args::AskArgs),
    Query(knowledge_args::QueryArgs),
    /// Ask typed questions (noul, choice, score) about a text or file via TypeSafe Jev; prints JSON
    Classify(classify_args::ClassifyArgs),
    Mcp,
    Hook(admin_args::HookArgs),
    Config(admin_args::ConfigArgs),
    Group(group_args::GroupArgs),
    Container(admin_args::ContainerArgs),
    /// Run cargo build/check and parse/deduplicate JSON compiler errors
    Build(cargo_args::BuildArgs),
    /// Run cargo test with trusted guarantees (zero-match is an error)
    Test(cargo_args::TestArgs),
    Worktree(project_args::WorktreeArgs),
    Store(admin_args::StoreArgs),
    Team(agent_provider_args::TeamArgs),
    Tool(agent_provider_args::ToolArgs),
    Doctor(admin_args::DoctorArgs),
    /// Manage BYOK providers (custom OpenAI-compatible endpoints) via opencode
    Byok(agent_provider_args::ByokArgs),
    Credential(agent_provider_args::CredentialArgs),
    Project(project_args::ProjectArgs),
    Memory(knowledge_args::MemoryArgs),
    /// Knowledge graph — temporal entity relationships
    Kg(knowledge_args::KgArgs),
    #[command(subcommand)]
    Experiment(ExperimentCommands),
    Upgrade(admin_args::UpgradeArgs),
    #[command(hide = true)]
    Init,
    Setup,
    #[command(hide = true, name = "__run-task")]
    InternalRunTask(project_args::InternalRunTaskArgs),
    Tree(inspect_args::TreeArgs),
    #[cfg(feature = "web")]
    #[command(name = "web")]
    Web(admin_args::WebArgs),
}
