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
        assert!(message.contains("Missing: aic binary not found"), "{message}");
        assert!(message.contains("curl -fsSL https://aid.agent-tools.org/install.sh | sh"));
    }
    assert!(default_audit.is_ok(), "project audit defaults must not refuse dispatch");
    assert!(available.is_ok());
}

#[path = "../../tests/fixtures/aic_shim.rs"]
mod aic_shim;

#[test]
fn audit_validation_explains_foreign_and_old_binaries() {
    let _guard = crate::task::aic::test_env_lock();
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("aic");
    unsafe { std::env::set_var("AIC_TEST_BINARY", &binary) };
    let args = crate::cmd::run::RunArgs {
        audit: true, audit_explicit: true, ..Default::default()
    };
    for (version, help, expected) in [
        ("aic 0.3.1", "AI-powered commit message generator", "Foreign"),
        ("aic 0.3.0", "Cross-audit aid agent outputs", "TooOld(aic 0.3.0)"),
    ] {
        crate::support::test_env::write_executable(
            &binary, &aic_shim::identified_script(version, help, "exit 0"),
        );
        let error = super::diagnostics::validate_run_options(&args).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("curl -fsSL https://aid.agent-tools.org/install.sh | sh"));
        if expected == "Foreign" {
            assert!(error.contains("cargo install aic installs an unrelated project"));
        }
    }
    crate::support::test_env::write_executable(&binary, &aic_shim::script("exit 0"));
    assert!(super::diagnostics::validate_run_options(&args).is_ok());
    unsafe { std::env::remove_var("AIC_TEST_BINARY") };
}
