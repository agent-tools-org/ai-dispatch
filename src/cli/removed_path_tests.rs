// Parser coverage for removed CLI paths and their surviving forms.
// Exercises clap directly; depends on the production Cli parser.

use super::Cli;
use clap::{Parser, error::ErrorKind};

fn assert_unknown_subcommand(args: &[&str], verb: &str) {
    let error = Cli::try_parse_from(args).err().expect("removed command accepted");
    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    assert!(error.to_string().contains(&format!("unrecognized subcommand '{verb}'")));
}

#[test]
fn kill_is_unknown() {
    assert_unknown_subcommand(&["aid", "kill", "t-1234"], "kill");
}

#[test]
fn summary_is_unknown() {
    assert_unknown_subcommand(&["aid", "summary", "wg-a"], "summary");
}

#[test]
fn finding_is_unknown() {
    assert_unknown_subcommand(&["aid", "finding", "list", "wg-a"], "finding");
}

#[test]
fn broadcast_is_unknown() {
    assert_unknown_subcommand(&["aid", "broadcast", "wg-a", "message"], "broadcast");
}

#[test]
fn output_is_unknown() {
    assert_unknown_subcommand(&["aid", "output", "t-1234"], "output");
}

#[test]
fn config_add_agent_is_unknown() {
    assert_unknown_subcommand(&["aid", "config", "add-agent", "local"], "add-agent");
}

#[test]
fn watch_rejects_wait_flag() {
    let error = Cli::try_parse_from(["aid", "watch", "--wait", "t-1234"])
        .err().expect("removed flag accepted");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    assert!(error.to_string().contains("unexpected argument '--wait'"));
}

#[test]
fn surviving_forms_parse() {
    for args in [
        vec!["aid", "stop", "t-1234", "--force"],
        vec!["aid", "group", "summary", "wg-a"],
        vec!["aid", "group", "finding", "add", "wg-a", "finding"],
        vec!["aid", "group", "finding", "list", "wg-a"],
        vec!["aid", "group", "finding", "get", "wg-a", "1"],
        vec!["aid", "group", "finding", "update", "wg-a", "1", "--verdict", "confirmed"],
        vec!["aid", "group", "broadcast", "wg-a", "message"],
        vec!["aid", "show", "t-1234", "--output", "--full"],
        vec!["aid", "agent", "add", "local"],
        vec!["aid", "group", "create", "release"],
        vec!["aid", "wait", "t-1234"],
    ] {
        assert!(Cli::try_parse_from(&args).is_ok(), "rejected {args:?}");
    }
}
