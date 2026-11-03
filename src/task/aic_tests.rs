// AIC identity, verdict and execution regressions.
// Uses isolated executable shims and the shared environment lock.

#[path = "../../tests/fixtures/aic_shim.rs"]
mod aic_shim;

use super::*;
use std::env;

fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    unsafe { env::set_var(key, value) }
}

fn remove_env(key: &str) {
    unsafe { env::remove_var(key) }
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::task::aic::test_env_lock()
}

fn install_aic_shim(dir: &Path, body: &str) {
    let path = dir.join("aic");
    crate::support::test_env::write_executable(&path, &aic_shim::script(body));
}

#[test]
fn is_available_caches_result() {
    let _guard = env_lock();
    set_env("AIC_TEST_PRESENT", "1");
    assert_eq!(is_available(), is_available());
    remove_env("AIC_TEST_PRESENT");
}

#[test]
fn run_audit_parses_pass_report_path() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "printf 'ok\\nreport: \
        /tmp/report.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));

    let result = run_audit("t-audit", Some(temp.path()));

    assert_eq!(result.verdict, "pass");
    assert_eq!(result.report_path.as_deref(), Some("/tmp/report.md"));
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn report_path_requires_terminal_stdout_line() {
    assert_eq!(
        report_path_from_stdout("report: /tmp/report.md\nsummary: complete\n"),
        None
    );
}

#[test]
fn run_audit_times_out() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    install_aic_shim(
        temp.path(),
        "sleep 2\nprintf 'report: \
        /tmp/late.md\\n'\nexit 0",
    );
    set_env("AIC_TEST_BINARY", temp.path().join("aic"));
    set_env("AID_AUDIT_TIMEOUT_SECS", "1");

    let result = run_audit("t-audit", Some(temp.path()));

    assert_eq!(result.verdict, "error");
    assert_eq!(result.report_path, None);
    remove_env("AID_AUDIT_TIMEOUT_SECS");
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn detects_identity_and_numeric_minimum_version() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("aic");
    set_env("AIC_TEST_BINARY", &binary);
    assert_eq!(status(), AicStatus::Missing);
    for (version, help, expected) in [
        ("aic 0.3.1", "AI-powered commit message generator", AicStatus::Foreign),
        ("aic 0.3.0", "Cross-audit aid agent outputs", AicStatus::TooOld("aic 0.3.0".into())),
        ("aic 0.10.0", "Cross-audit aid agent outputs", AicStatus::Ready),
        ("aic 0.3.1", "Cross-audit aid agent outputs", AicStatus::Ready),
        ("aic invalid", "Cross-audit aid agent outputs", AicStatus::TooOld("aic invalid".into())),
        ("aic 0.3.1", "prefix Cross-audit aid agent outputs", AicStatus::Foreign),
        ("aic 0.3.1", "Cross-audit aid agent outputs suffix", AicStatus::Foreign),
    ] {
        crate::support::test_env::write_executable(
            &binary, &aic_shim::identified_script(version, help, "exit 0"),
        );
        assert_eq!(status(), expected);
        assert_eq!(is_available(), expected == AicStatus::Ready);
    }
    crate::support::test_env::write_executable(&binary, "#!/bin/sh\nexit 1\n");
    assert_eq!(status(), AicStatus::Missing);
    remove_env("AIC_TEST_BINARY");
}

#[test]
fn supported_versions_require_three_numeric_components() {
    for version in ["aic 0.3.1", "aic 0.10.0", "aic 1.0.0"] {
        assert!(supported_version(version), "{version}");
    }
    for version in ["aic 0.3.0", "aic 0.2.99", "aic 0.3", "aic 0.3.1.0",
        "aic 0.3.1-beta", "other 1.0.0", "aic +1.0.0", "aic 1..0"]
    {
        assert!(!supported_version(version), "{version}");
    }
}
