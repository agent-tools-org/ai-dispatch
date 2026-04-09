// CLI tests for export-specific flag parsing.
// Exports: module-local tests for src/cli command parsing.
// Deps: Cli, Commands, command_args_b, clap::Parser.
use super::{Cli, Commands, command_args_b};
use clap::Parser;

#[test]
fn export_sharegpt_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "export", "--sharegpt", "t-1234"]).unwrap();
    match cli.command {
        Some(Commands::Export(command_args_b::ExportArgs { task_id, sharegpt, .. })) => {
            assert_eq!(task_id, "t-1234");
            assert!(sharegpt);
        }
        _ => panic!("expected Export"),
    }
}
