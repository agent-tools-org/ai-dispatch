// aid CLI arguments for controlling tasks: retry, merge, accept/reject, gc, respond, reply,
// stop, steer, unstick.
// Exports the matching clap Args structs; depends on clap derive.

use clap::Args;

#[derive(Args)]
pub struct RetryArgs {
    pub task_id: String,
    #[arg(short, long, conflicts_with = "feedback_file")]
    pub feedback: Option<String>,
    #[arg(long, short = 'F', conflicts_with = "feedback")]
    pub feedback_file: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, value_name = "SECS")]
    pub idle_timeout: Option<u64>,
    #[arg(long)]
    pub dir: Option<String>,
    #[arg(long)]
    pub reset: bool,
    #[arg(long, help = "Run in background (non-blocking)")]
    pub bg: bool,
}

#[derive(Args)]
pub struct MergeArgs {
    pub task_id: Option<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long)]
    pub approve: bool,
    #[arg(long)]
    pub check: bool,
    #[arg(long, help = "Allow merging a task in FAIL or STOPPED state (use when the task's code changes are good but verify failed).")]
    pub force: bool,
    #[arg(long, help = "Merge into this branch instead of current")]
    pub target: Option<String>,
    #[arg(long, help = "Apply group task branches as GitButler virtual branch lanes")]
    pub lanes: bool,
}

#[derive(Args)]
pub struct ArtifactDecisionArgs {
    /// Task whose delivered artifacts are being accepted or rejected.
    pub task_id: String,
}

#[derive(Args)]
pub struct ArtifactGcArgs {
    /// Accepted task whose worktree may be deleted after durability proof.
    #[arg(long)]
    pub task: String,
}

#[derive(Args)]
pub struct RespondArgs {
    pub task_id: String,
    pub input: Option<String>,
    #[arg(long, short = 'F')]
    pub file: Option<String>,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid reply t-1234 "Need status update"
  aid reply t-1234 -F reply.md
  aid reply t-1234 "continue" --async
  aid reply t-1234 "status?" --timeout 60"#)]
pub struct ReplyArgs {
    pub task_id: String,
    pub message: Option<String>,
    #[arg(long, short = 'F')]
    pub file: Option<String>,
    #[arg(long = "async")]
    pub async_mode: bool,
    #[arg(long = "timeout", value_name = "SECS", default_value = "30", help = "Wait this many seconds for an acknowledgement")]
    pub timeout_secs: u64,
}

#[derive(Args)]
pub struct StopArgs {
    pub task_id: String,
    #[arg(long)]
    pub force: bool,
    /// Stop the entire retry tree containing this task — root + every retry
    /// descendant in a non-terminal state. The argument may be the root or
    /// any task in the chain; aid resolves to the root automatically.
    #[arg(long = "retry-tree")]
    pub retry_tree: bool,
}

#[derive(Args)]
pub struct SteerArgs {
    pub task_id: String,
    pub message: String,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid unstick t-1234
  aid unstick t-1234 -m "Please summarize current blocker"
  aid unstick t-1234 --escalate"#)]
pub struct UnstickArgs {
    pub task_id: String,
    #[arg(long, short = 'm')]
    pub message: Option<String>,
    #[arg(long)]
    pub escalate: bool,
}
