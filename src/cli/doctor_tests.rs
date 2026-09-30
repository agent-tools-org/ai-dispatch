// Parser coverage for the `aid doctor` CLI surface.
// Confirms the top-level command and `--apply` flag are wired through clap.
// Deps: clap Parser and the local cli module exports.

use super::{Cli, Commands, admin_args};
use clap::Parser;

#[test]
fn doctor_command_parses() {
    let cli = Cli::try_parse_from(["aid", "doctor", "--apply"]).expect("doctor command parses");
    match cli.command {
        Some(Commands::Doctor(admin_args::DoctorArgs { apply })) => assert!(apply),
        _ => panic!("expected Doctor"),
    }
}
