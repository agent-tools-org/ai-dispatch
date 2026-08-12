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

#[test]
fn retry_model_and_idle_timeout_parse() {
    let cli = Cli::try_parse_from([
        "aid",
        "retry",
        "t-1234",
        "-f",
        "fix it",
        "--model",
        "gpt-5.4",
        "--idle-timeout",
        "900",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Retry(command_args_b::RetryArgs {
            model,
            idle_timeout,
            feedback,
            feedback_file,
            ..
        })) => {
            assert_eq!(model.as_deref(), Some("gpt-5.4"));
            assert_eq!(idle_timeout, Some(900));
            assert_eq!(feedback.as_deref(), Some("fix it"));
            assert!(feedback_file.is_none());
        }
        _ => panic!("expected Retry"),
    }
}

#[test]
fn retry_feedback_file_short_flag_uses_capital_f() {
    let cli = Cli::try_parse_from(["aid", "retry", "t-1234", "-F", "notes.md"]).unwrap();
    match cli.command {
        Some(Commands::Retry(command_args_b::RetryArgs {
            feedback,
            feedback_file,
            ..
        })) => {
            assert!(feedback.is_none());
            assert_eq!(feedback_file.as_deref(), Some("notes.md"));
        }
        _ => panic!("expected Retry"),
    }
}

#[test]
fn retry_rejects_both_feedback_flags() {
    let err = match Cli::try_parse_from([
        "aid",
        "retry",
        "t-1234",
        "-f",
        "inline",
        "-F",
        "notes.md",
    ]) {
        Ok(_) => panic!("expected clap to reject conflicting feedback flags"),
        Err(err) => err,
    };
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("cannot be used with") || msg.contains("conflict"),
        "unexpected error: {msg}"
    );
}
