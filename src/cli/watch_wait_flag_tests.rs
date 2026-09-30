// E2E-style parser tests for watch and wait flags.
// Covers clap try_parse behavior for the blocking command and global quiet.

use super::{Cli, Commands, command_args_watch};
use clap::Parser;

#[test]
fn watch_subcommand_accepts_global_quiet_short_flag() {
    let cli = Cli::try_parse_from(["aid", "watch", "-q", "t-1234"]).unwrap();
    assert!(cli.quiet);
    match cli.command {
        Some(Commands::Watch(command_args_watch::WatchArgs { task_ids, .. })) => {
            assert_eq!(task_ids, vec!["t-1234".to_string()]);
        }
        _ => panic!("expected Watch"),
    }
}

#[test]
fn wait_subcommand_parses_timeout_and_exit_on_await() {
    let cli = Cli::try_parse_from([
        "aid",
        "wait",
        "--timeout",
        "60",
        "--exit-on-await",
        "t-1234",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Wait(command_args_watch::WaitArgs {
            task_ids,
            timeout,
            exit_on_await,
            ..
        })) => {
            assert_eq!(task_ids, vec!["t-1234".to_string()]);
            assert_eq!(timeout, Some(60));
            assert!(exit_on_await);
        }
        _ => panic!("expected Wait"),
    }
}
