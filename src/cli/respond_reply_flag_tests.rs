// Parser tests for `aid respond`/`aid reply` `--file` short-flag (-F).
// Verifies -F maps to --file and that the old -f spelling is no longer accepted
// on these subcommands (it conflicts with retry's -f = --feedback).

use super::{Cli, Commands, command_args_b};
use clap::Parser;

#[test]
fn respond_file_short_flag_uses_capital_f() {
    let cli = Cli::try_parse_from(["aid", "respond", "t-1", "-F", "reply.md"]).unwrap();
    match cli.command {
        Some(Commands::Respond(command_args_b::RespondArgs { task_id, file, .. })) => {
            assert_eq!(task_id, "t-1");
            assert_eq!(file.as_deref(), Some("reply.md"));
        }
        _ => panic!("expected Respond"),
    }
}

#[test]
fn respond_rejects_lowercase_f_for_file() {
    let err = match Cli::try_parse_from(["aid", "respond", "t-1", "-f", "reply.md"]) {
        Ok(_) => panic!("expected respond to reject -f"),
        Err(err) => err,
    };
    assert!(err.to_string().to_lowercase().contains("unexpected argument"));
}

#[test]
fn reply_file_short_flag_uses_capital_f() {
    let cli = Cli::try_parse_from(["aid", "reply", "t-1", "-F", "reply.md"]).unwrap();
    match cli.command {
        Some(Commands::Reply(command_args_b::ReplyArgs { task_id, file, .. })) => {
            assert_eq!(task_id, "t-1");
            assert_eq!(file.as_deref(), Some("reply.md"));
        }
        _ => panic!("expected Reply"),
    }
}

#[test]
fn reply_rejects_lowercase_f_for_file() {
    let err = match Cli::try_parse_from(["aid", "reply", "t-1", "-f", "reply.md"]) {
        Ok(_) => panic!("expected reply to reject -f"),
        Err(err) => err,
    };
    assert!(err.to_string().to_lowercase().contains("unexpected argument"));
}