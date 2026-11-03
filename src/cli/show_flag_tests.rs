// E2E-style parser tests for `aid show` mode flags.
// Covers mutually exclusive modes and events-only parsing.

use super::{Cli, Commands, inspect_args};
use clap::Parser;

#[test]
fn show_events_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--events"]).unwrap();
    match cli.command {
        Some(Commands::Show(inspect_args::ShowArgs { task_id, events, .. })) => {
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

#[test]
fn show_patch_parses_and_conflicts_with_other_modes() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--patch"]).unwrap();
    assert!(matches!(cli.command,
        Some(Commands::Show(inspect_args::ShowArgs { patch: true, .. }))));
    for flag in ["--events", "--context", "--diff", "--summary", "--branch", "--output",
        "--result", "--transcript", "--full", "--brief", "--explain", "--log", "--json"]
    {
        let error = Cli::try_parse_from(["aid", "show", "t-1234", "--patch", flag])
            .err().expect("patch mode must conflict");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict, "{flag}: {error}");
    }
    for flag in ["--file", "--agent", "--model"] {
        assert!(Cli::try_parse_from(["aid", "show", "t-1234", "--patch", flag, "value"]).is_err());
    }
}
