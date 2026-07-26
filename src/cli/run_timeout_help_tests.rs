// Focused generated-help tests for timeout-related CLI flags.
// Exports: none; loaded by `cli/mod.rs` under `#[cfg(test)]`.
// Deps: clap CommandFactory and the top-level CLI parser.

use super::Cli;
use clap::CommandFactory;

#[test]
fn run_timeout_help_names_seconds_unit() {
    let mut cmd = Cli::command();
    let run = cmd.find_subcommand_mut("run").expect("run subcommand");
    let help = run.render_long_help().to_string();

    assert!(help.contains("--timeout <SECS>"), "got: {help}");
    assert!(help.contains("Hard cap for the run in seconds"), "got: {help}");
    assert!(help.contains("for minutes, multiply by 60"), "got: {help}");
}
