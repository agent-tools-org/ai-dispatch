// aid CLI run extras.
// Exports RunExtrasArgs; depends on clap derive.

use clap::Args;

#[derive(Args)]
pub(crate) struct RunExtrasArgs {
    /// Inject output from previous task(s) as context
    #[arg(long, num_args(1..))]
    pub(crate) context_from: Vec<String>,
    /// Methodology skills to inject
    #[arg(long, num_args(1..))]
    pub(crate) skill: Vec<String>,
    /// Prompt template to wrap around the task
    #[arg(long)]
    pub(crate) template: Option<String>,
    /// Command to run on task completion
    #[arg(long)]
    pub(crate) on_done: Option<String>,
    /// Agent cascade: comma-separated list of agents to try on failure (e.g. opencode,codex,cursor)
    #[arg(long, value_delimiter = ',')]
    pub(crate) cascade: Vec<String>,
    /// Hook specs to run for the dispatched task
    #[arg(long)]
    pub(crate) hook: Vec<String>,
}
