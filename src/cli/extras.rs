// aid CLI run extras.
// Exports RunExtrasArgs; depends on clap derive.

use clap::Args;

#[derive(Args)]
pub(crate) struct RunExtrasArgs {
    /// Run Cargo builds on an rbox build box (bare flag selects automatically)
    #[arg(long, num_args = 0..=1, default_missing_value = "auto", value_name = "BOX", conflicts_with_all = ["sandbox", "container"])]
    pub(crate) remote_build: Option<String>,
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
    /// Back up task artifacts when the task ends: TARGET[:FOLDER], e.g. gdrive:audits/{project}
    #[arg(long, value_name = "TARGET[:FOLDER]", conflicts_with = "no_backup")]
    pub(crate) backup: Option<String>,
    /// Skip artifact backup for this task even when the project configures one
    #[arg(long)]
    pub(crate) no_backup: bool,
}

#[cfg(test)]
mod tests {
    use super::RunExtrasArgs;
    use clap::Parser;

    /// `--remote-build` conflicts with these, so the probe must declare them.
    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        extras: RunExtrasArgs,
        #[arg(long)]
        sandbox: bool,
        #[arg(long)]
        container: Option<String>,
    }

    #[test]
    fn backup_flag_takes_target_and_optional_folder() {
        let probe = Probe::try_parse_from(["aid", "--backup", "gdrive:audits/{project}"]).unwrap();
        assert_eq!(probe.extras.backup.as_deref(), Some("gdrive:audits/{project}"));
        assert!(!probe.extras.no_backup);
        let probe = Probe::try_parse_from(["aid", "--no-backup"]).unwrap();
        assert!(probe.extras.no_backup);
    }

    #[test]
    fn backup_and_no_backup_conflict() {
        assert!(Probe::try_parse_from(["aid", "--backup", "gdrive", "--no-backup"]).is_err());
    }
}
