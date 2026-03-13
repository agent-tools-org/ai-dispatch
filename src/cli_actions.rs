// Shared clap subcommand enums used by the main CLI entrypoint.
// Keeps main.rs focused on command wiring rather than enum definitions.
// Depends on clap derive macros only.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ConfigAction {
    /// List configured agents
    Agents,
    /// Register custom agent
    AddAgent {
        name: String,
        command: String,
        #[arg(long)]
        streaming: bool,
    },
    /// Clear rate-limit marker for an agent (or "all")
    ClearLimit {
        /// Agent name (e.g. codex, gemini) or "all"
        agent: String,
    },
    /// Show pricing table
    Pricing,
    /// List available skills
    Skills,
    /// List available templates
    Templates,
}

#[derive(Subcommand)]
pub enum GroupAction {
    /// Create a workgroup with shared context
    Create {
        name: String,
        #[arg(long)]
        context: String,
    },
    /// List workgroups
    List,
    /// Show one workgroup and its member tasks
    Show {
        group_id: String,
    },
    /// Update a workgroup name and/or shared context
    Update {
        group_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        context: Option<String>,
    },
    /// Delete a workgroup definition
    Delete {
        group_id: String,
    },
}
