// Focused CLI parser tests for audit-related `aid run` flags.
// Exports: none; loaded by `cli/mod.rs` under `#[cfg(test)]`.
// Deps: clap Parser and the CLI command arg types.

use super::{Cli, Commands, command_args_a};
use clap::Parser;

#[test]
fn run_no_audit_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--no-audit"]).unwrap();
    match cli.command {
        Some(Commands::Run(command_args_a::RunArgs { no_audit, .. })) => assert!(no_audit),
        _ => panic!("expected Run"),
    }
}
