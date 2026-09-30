// aid CLI arguments for workgroups.
// Exports GroupArgs, GroupAction, FindingCommands; depends on clap derive.

use clap::{Args, Subcommand};

#[derive(Args)]
pub struct GroupArgs {
    #[command(subcommand)]
    pub action: GroupAction,
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
        #[arg(long)]
        cascade: bool,
    },
    /// Cancel all non-terminal tasks in a workgroup
    Cancel {
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
        action: crate::cli::FindingCommands,
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
#[allow(clippy::large_enum_variant)]
pub enum FindingCommands {
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
        #[arg(long)]
        severity: Option<String>,
        #[arg(long)]
        verdict: Option<String>,
    },
    /// Show a single finding for a workgroup
    Get {
        /// Workgroup ID
        group: String,
        /// Finding ID
        finding_id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Update review metadata for a finding
    Update {
        /// Workgroup ID
        group: String,
        /// Finding ID
        finding_id: i64,
        #[arg(long)]
        verdict: Option<String>,
        #[arg(long)]
        score: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
}
