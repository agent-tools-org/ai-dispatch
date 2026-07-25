// Parser coverage for the `aid build` CLI surface.
// Confirms typed command, package, warnings, test filter, and trailing cargo args.
// Deps: clap Parser and the local cli module exports.

use super::command_args_b::{BuildCommandArg, BuildArgs};
use super::{Cli, Commands};
use clap::Parser;

#[test]
fn build_command_parses_typed_options() {
    let cli = Cli::try_parse_from([
        "aid",
        "build",
        "test",
        "-p",
        "ai-dispatch",
        "--test",
        "retry_flow",
        "--warnings",
        "--",
        "--all-targets",
    ])
    .expect("build command parses");
    match cli.command {
        Some(Commands::Build(BuildArgs {
            command,
            package,
            test_filter,
            warnings,
            extra_args,
        })) => {
            assert_eq!(command, Some(BuildCommandArg::Test));
            assert_eq!(package.as_deref(), Some("ai-dispatch"));
            assert_eq!(test_filter.as_deref(), Some("retry_flow"));
            assert!(warnings);
            assert_eq!(extra_args, ["--all-targets"]);
        }
        _ => panic!("expected Build"),
    }
}

#[test]
fn build_rejects_unsupported_passthrough_command() {
    let result = Cli::try_parse_from(["aid", "build", "build"]);
    assert!(result.is_err());
}
