// aid CLI arguments for administration: config, hook, store, container, doctor, upgrade, web.
// Exports the matching clap Args structs and their subcommand enums; depends on clap derive.

use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// List configured agents
    Agents,
    /// Clear rate-limit marker for an agent (or "all")
    ClearLimit {
        /// Agent name (e.g. codex, gemini) or "all"
        agent: String,
    },
    /// Show pricing table
    Pricing {
        #[arg(long)]
        update: bool,
    },
    /// List available skills
    Skills,
    /// Display skill token estimates for prompt budgeting
    PromptBudget,
    /// List available templates
    Templates,
}

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub action: HookAction,
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Print session-start hook text for Claude Code
    SessionStart,
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

#[derive(Subcommand)]
pub enum StoreCommands {
    /// Browse available agents in the store
    Browse {
        /// Optional search query to filter agents
        query: Option<String>,
    },
    /// Install an agent from the store (publisher/name)
    Install { name: String },
    /// Show agent TOML from the store (publisher/name)
    Show { name: String },
    /// Check for updates to installed store packages
    Update {
        /// Apply available updates
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Args)]
pub struct ContainerArgs {
    #[command(subcommand)]
    pub action: ContainerAction,
}

#[derive(Subcommand)]
pub enum ContainerAction {
    /// Build a container image from a Containerfile
    Build {
        tag: String,
        #[arg(long)]
        file: Option<String>,
    },
    /// List running dev containers
    List,
    /// Stop and remove a dev container
    Stop {
        name: String,
    },
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid doctor
  aid doctor --apply"#)]
pub struct DoctorArgs {
    #[arg(long)]
    pub apply: bool,
}

#[derive(Args)]
pub struct UpgradeArgs {
    #[arg(long)]
    pub force: bool,
}

#[cfg(feature = "web")]
#[derive(Args)]
pub struct WebArgs {
    #[arg(long, default_value = "8080")]
    pub port: u16,
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long)]
    pub token: Option<String>,
}
