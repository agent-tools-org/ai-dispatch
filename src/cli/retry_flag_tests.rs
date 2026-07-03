// Retry CLI parser tests.
// Covers retry-specific flags; depends on clap Parser and cli module exports.

use super::{Cli, Commands, command_args_b};
use clap::Parser;

#[test]
fn retry_bg_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "retry", "t-1234", "-f", "fix it", "--bg"]).unwrap();
    match cli.command {
        Some(Commands::Retry(command_args_b::RetryArgs { bg, .. })) => assert!(bg),
        _ => panic!("expected Retry"),
    }
}
