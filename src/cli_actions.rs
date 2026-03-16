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
    /// Display skill token estimates for prompt budgeting
    PromptBudget,
    /// List available templates
    Templates,
}

#[derive(Subcommand)]
pub enum GroupAction {
    /// Create a workgroup
    Create {
        /// Workgroup name
        name: String,
        /// Shared context files (e.g. src/types.rs)
        #[arg(long, short)]
        context: Option<String>,
        /// Custom workgroup ID (default: auto-generated wg-xxxx)
        #[arg(long)]
        id: Option<String>,
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

#[derive(Subcommand)]
pub enum TeamAction {
    /// List all teams
    List,
    /// Show team details and members
    Show {
        /// Team name
        name: String,
    },
    /// Create a new team definition
    Create {
        /// Team name
        name: String,
    },
    /// Remove a team definition
    Delete {
        /// Team name
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ProjectAction {
    /// Initialize project configuration in current repo
    Init,
    /// Show the detected project configuration
    Show,
}

#[derive(Subcommand)]
pub enum WorktreeAction {
    /// Create a worktree for a branch (prints path to stdout)
    Create {
        /// Branch name for the worktree
        branch: String,
        /// Base branch to fork from (default: HEAD)
        #[arg(long)]
        base: Option<String>,
        /// Repository path (defaults to current dir)
        #[arg(long)]
        repo: Option<String>,
    },
    /// List active aid-managed worktrees
    List {
        /// Repository path (defaults to current dir)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Remove a worktree and prune git references
    Remove {
        /// Branch name of the worktree to remove
        branch: String,
        /// Repository path (defaults to current dir)
        #[arg(long)]
        repo: Option<String>,
    },
}
