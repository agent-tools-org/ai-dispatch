// Parser coverage for the `aid doctor` CLI surface.
// Confirms the top-level command and `--apply` flag are wired through clap.
// Deps: clap Parser and the local cli module exports.

use super::{Cli, Commands, command_args_c};
use clap::Parser;

#[test]
fn doctor_command_parses() {
    let cli = Cli::try_parse_from(["aid", "doctor", "--apply"]).unwrap();
    match cli.command {
        Some(Commands::Doctor(command_args_c::DoctorArgs { apply })) => assert!(apply),
        _ => panic!("expected Doctor"),
    }
}
