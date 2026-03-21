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
    /// Summarize workgroup results with milestones, findings, costs
    Summary {
        /// Workgroup ID (e.g. wg-abc1)
        group_id: String,
    },
    /// Post or list workgroup findings
    Finding {
        #[command(subcommand)]
        action: GroupFindingAction,
    },
    /// Send a message to the workgroup's broadcast channel
    Broadcast {
        /// Workgroup ID
        group_id: String,
        /// Message to broadcast
        message: String,
    },
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

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum GroupFindingAction {
    /// Post a finding to a workgroup
    Add {
        /// Workgroup ID
        group: String,
        /// Finding content
        content: Option<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        file: Option<String>,
        /// Source task ID (optional)
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        severity: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, name = "finding-file")]
        finding_file: Option<String>,
        #[arg(long)]
        lines: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        confidence: Option<String>,
    },
    /// List findings for a workgroup
    List {
        /// Workgroup ID
        group: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        count: bool,
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
    /// Show the current computed project state
    State,
    /// Sync project config to CLAUDE.md and global budget
    Sync,
}

#[derive(Subcommand)]
pub enum ToolAction {
    /// List available tools
    List {
        /// Filter to a specific team
        #[arg(long)]
        team: Option<String>,
    },
    /// Show tool details
    Show {
        name: String,
        /// Search in team tools directory
        #[arg(long)]
        team: Option<String>,
    },
    /// Create a new tool definition
    Add {
        name: String,
        /// Create in team tools directory
        #[arg(long)]
        team: Option<String>,
    },
    /// Remove a tool definition
    Remove { name: String },
    /// Test-run a tool with arguments
    Test {
        name: String,
        /// Search in team tools directory
        #[arg(long)]
        team: Option<String>,
        /// Arguments to pass to the tool
        args: Vec<String>,
    },
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
    /// Prune stale aid-managed worktrees
    Prune {
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
