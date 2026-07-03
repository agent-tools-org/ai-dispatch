// E2E-style parser tests for `aid show` mode flags.
// Covers mutually exclusive modes and events-only parsing.

use super::{Cli, Commands, command_args_a};
use clap::Parser;

#[test]
fn show_events_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--events"]).unwrap();
    match cli.command {
        Some(Commands::Show(command_args_a::ShowArgs { task_id, events, .. })) => {
            assert_eq!(task_id, "t-1234");
            assert!(events);
        }
        _ => panic!("expected Show"),
    }
}

#[test]
fn conflicting_show_modes_are_rejected() {
    let err = match Cli::try_parse_from(["aid", "show", "t-1234", "--output", "--log"]) {
        Ok(_) => panic!("expected show mode conflict"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("cannot be used with"));
}
