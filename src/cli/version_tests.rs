// Build-provenance tests for `aid --version`.
// Exports: none; loaded by `cli/mod.rs` under `#[cfg(test)]`.
// Deps: clap CommandFactory and the top-level CLI parser.

use super::Cli;
use clap::CommandFactory;

#[test]
fn version_string_starts_with_crate_version() {
    let cmd = Cli::command();
    let version = cmd.get_version().expect("version should be set");
    assert!(
        version.starts_with(env!("CARGO_PKG_VERSION")),
        "got: {version}"
    );
}

#[test]
fn version_string_carries_a_parenthesized_provenance_field() {
    let cmd = Cli::command();
    let version = cmd.get_version().expect("version should be set");
    // Provenance is always present: a git describe/short-SHA (optionally
    // "-dirty"), or the literal "no git metadata" when built without git.
    // Either way the field must never be blank or silently omitted.
    let provenance = version
        .strip_prefix(env!("CARGO_PKG_VERSION"))
        .and_then(|rest| rest.trim().strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("expected \"<version> (<provenance>)\", got: {version}"));
    assert!(!provenance.is_empty(), "got: {version}");
}
