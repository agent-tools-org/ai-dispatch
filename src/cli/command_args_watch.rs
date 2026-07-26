// Watch and wait CLI argument structs.
// Exports clap Args types for blocking and live task monitoring commands.

use clap::Args;

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid watch t-1234                # Live task view
  aid watch --wait t-1234         # Block until done (--quiet alias is deprecated)
  aid watch --stream --group wg-a # JSONL events
  aid watch --tui                 # Full dashboard TUI"#)]
pub struct WatchArgs {
    pub task_ids: Vec<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long, conflicts_with_all = ["wait", "stream", "exit_on_await", "timeout"])]
    pub tui: bool,
    #[arg(long, conflicts_with_all = ["tui", "stream"])]
    pub wait: bool,
    #[arg(long, conflicts_with_all = ["tui", "wait", "exit_on_await"])]
    pub stream: bool,
    #[arg(long, conflicts_with_all = ["tui", "stream"])]
    pub exit_on_await: bool,
    #[arg(long, value_name = "SECS", conflicts_with = "tui", help = "Stop waiting after this many seconds")]
    pub timeout: Option<u64>,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid wait t-1234
  aid wait --group wg-a --timeout 60
  aid wait --exit-on-await t-1234"#)]
pub struct WaitArgs {
    pub task_ids: Vec<String>,
    #[arg(long)]
    pub group: Option<String>,
    #[arg(long)]
    pub exit_on_await: bool,
    #[arg(long, value_name = "SECS", help = "Stop waiting after this many seconds")]
    pub timeout: Option<u64>,
}
