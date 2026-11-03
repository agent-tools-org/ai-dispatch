// Focused CLI parser tests for audit-related `aid run` flags.
// Exports: none; loaded by `cli/mod.rs` under `#[cfg(test)]`.
// Deps: clap Parser and the CLI command arg types.

use super::{Cli, Commands, run_args};
use clap::Parser;

#[test]
fn run_no_audit_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--no-audit"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { no_audit, .. })) => assert!(no_audit),
        _ => panic!("expected Run"),
    }
}

#[test]
fn audit_validation_requires_available_aic() {
    let _guard = crate::task::aic::test_env_lock();
    unsafe { std::env::set_var("AIC_TEST_PRESENT", "0") };
    let mut args = crate::cmd::run::RunArgs::default();
    assert!(super::diagnostics::validate_run_options(&args).is_ok());
    args.audit = true;
    let default_audit = super::diagnostics::validate_run_options(&args);
    args.audit_explicit = true;
    let explicit = super::diagnostics::validate_run_options(&args);
    args.audit_explicit = false;
    args.declared_rigor = Some(crate::types::TaskRigor::Critical);
    args.verify = Some("true".into());
    let critical = super::diagnostics::validate_run_options(&args);
    unsafe { std::env::set_var("AIC_TEST_PRESENT", "1") };
    let available = super::diagnostics::validate_run_options(&args);
    unsafe { std::env::remove_var("AIC_TEST_PRESENT") };
    for result in [explicit, critical] {
        let message = result.expect_err("missing aic must reject dispatch").to_string();
        assert!(message.contains("aic"), "{message}");
        assert!(message.contains("audit_unavailable"), "{message}");
    }
    assert!(default_audit.is_ok(), "project audit defaults must not refuse dispatch");
    assert!(available.is_ok());
}
