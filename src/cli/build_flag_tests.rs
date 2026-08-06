// Parser coverage for the `aid build` and `aid test` CLI surfaces.
// Confirms typed command, package, warnings, test filter, and trailing cargo args.
// Deps: clap Parser and the local cli module exports.

use super::command_args_b::{BuildCommandArg, BuildArgs, TestArgs};
use super::{Cli, Commands};
use clap::Parser;

#[test]
fn build_command_parses_typed_options() {
    let cli = Cli::try_parse_from([
        "aid",
        "build",
        "check",
        "-p",
        "ai-dispatch",
        "--warnings",
        "--",
        "--all-targets",
    ])
    .expect("build command parses");
    match cli.command {
        Some(Commands::Build(BuildArgs {
            command,
            package,
            warnings,
            extra_args,
        })) => {
            assert_eq!(command, Some(BuildCommandArg::Check));
            assert_eq!(package.as_deref(), Some("ai-dispatch"));
            assert!(warnings);
            assert_eq!(extra_args, ["--all-targets"]);
        }
        _ => panic!("expected Build"),
    }
}

#[test]
fn build_rejects_test_subcommand() {
    let result = Cli::try_parse_from(["aid", "build", "test"]);
    assert!(result.is_err());
}

#[test]
fn build_rejects_unsupported_passthrough_command() {
    let result = Cli::try_parse_from(["aid", "build", "build"]);
    assert!(result.is_err());
}

#[test]
fn test_command_parses_filter_bin_and_harness_args() {
    let cli = Cli::try_parse_from([
        "aid",
        "test",
        "--bin",
        "aid",
        "--isolated",
        "paths::aid_dir",
        "--",
        "--exact",
    ])
    .expect("test command parses");
    match cli.command {
        Some(Commands::Test(TestArgs {
            package,
            bin,
            lib,
            test_target,
            filter,
            isolated,
            warnings,
            extra_args,
        })) => {
            assert!(package.is_none());
            assert_eq!(bin.as_deref(), Some("aid"));
            assert!(!lib);
            assert!(test_target.is_none());
            assert_eq!(filter.as_deref(), Some("paths::aid_dir"));
            assert!(isolated);
            assert!(!warnings);
            assert_eq!(extra_args, ["--exact"]);
        }
        _ => panic!("expected Test"),
    }
}

#[test]
fn test_command_parses_filter_after_double_dash() {
    // cargo muscle memory: `aid test -- name` — filter lands in extra_args.
    let cli = Cli::try_parse_from(["aid", "test", "--", "nonexistent_test_xyz", "--exact"])
        .expect("test command parses");
    match cli.command {
        Some(Commands::Test(TestArgs {
            filter,
            extra_args,
            test_target,
            ..
        })) => {
            assert!(filter.is_none());
            assert!(test_target.is_none());
            assert_eq!(extra_args, ["nonexistent_test_xyz", "--exact"]);
        }
        _ => panic!("expected Test"),
    }
}

#[test]
fn test_command_test_flag_is_target_not_name_filter() {
    let cli = Cli::try_parse_from(["aid", "test", "--test", "template_e2e"])
        .expect("test command parses");
    match cli.command {
        Some(Commands::Test(TestArgs {
            filter,
            test_target,
            ..
        })) => {
            assert!(filter.is_none());
            assert_eq!(test_target.as_deref(), Some("template_e2e"));
        }
        _ => panic!("expected Test"),
    }
}
