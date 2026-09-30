// aid CLI arguments for cargo wrappers: build and test.
// Exports BuildArgs, BuildCommandArg, TestArgs; depends on clap derive.

use clap::{Args, ValueEnum};

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid build
  aid build check
  aid build clippy -- --all-targets

For trusted test runs (zero-match is an error, named executed tests), use `aid test`."#)]
pub struct BuildArgs {
    /// Cargo verification command. Defaults to project verify config, then check.
    /// Project verify of `cargo test …` maps to check; use `aid test` for tests.
    #[arg(value_enum)]
    pub command: Option<BuildCommandArg>,
    /// Cargo package to verify.
    #[arg(short = 'p', long)]
    pub package: Option<String>,
    /// Include warning diagnostics instead of reporting only their count.
    #[arg(long)]
    pub warnings: bool,
    /// Extra cargo arguments appended after aid's generated arguments.
    #[arg(last = true, allow_hyphen_values = true)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildCommandArg {
    Check,
    Clippy,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid test
  aid test --bin aid
  aid test --bin aid paths::aid_dir -- --exact
  aid test -- my_filter
  aid test --lib
  aid test --isolated --bin aid my_test

Target selectors are aid flags (`--lib`, `--bin`, `--test NAME` = target, not
name filter). Free args after `--` are harness args (filter / --exact / …).
Guarantees: zero-match filter fails (positional or after `--`); no targets
never looks like a pass; digest names tests; failures stay compact."#)]
pub struct TestArgs {
    /// Cargo package (-p).
    #[arg(short = 'p', long)]
    pub package: Option<String>,
    /// Only run tests for this binary target.
    #[arg(long)]
    pub bin: Option<String>,
    /// Only run library unit tests (`cargo test --lib`).
    #[arg(long)]
    pub lib: bool,
    /// Integration test target (`cargo test --test NAME`). Not a name filter.
    #[arg(long = "test")]
    pub test_target: Option<String>,
    /// Name filter; also free after `--` (`aid test -- name`).
    pub filter: Option<String>,
    /// Temporary AID_HOME for the cargo child (no ~/.aid read/write).
    #[arg(long)]
    pub isolated: bool,
    /// Include warning diagnostics instead of only their count.
    #[arg(long)]
    pub warnings: bool,
    /// Args after `--` go to the test harness (filter, --exact, …).
    #[arg(last = true, allow_hyphen_values = true)]
    pub extra_args: Vec<String>,
}
