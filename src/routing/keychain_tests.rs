// Tests for keychain key reads: argument shape, trimming, refusals, and no secret in errors.
// Exports: none. Deps: super, crate::support::test_env::write_executable, tempfile.

use super::*;
use crate::support::test_env::write_executable;

const SECRET: &str = "sk-ant-test-0123456789";

fn item() -> KeychainItem {
    KeychainItem { service: "aid-anthropic-api".into(), account: "claude-api-meet".into() }
}

fn fake_security(dir: &Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("security");
    write_executable(&path, &format!("#!/bin/sh\n{body}\n"));
    path
}

fn read_env(key: ApiKey) -> String {
    let mut cmd = Command::new("/usr/bin/printenv");
    cmd.arg("AID_TEST_KEY").env_remove("AID_TEST_KEY");
    key.apply_to(&mut cmd, "AID_TEST_KEY");
    let out = cmd.output().expect("printenv");
    String::from_utf8(out.stdout).expect("utf8")
}

#[test]
fn passes_service_and_account_and_trims_the_newline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let argv = dir.path().join("argv");
    let program = fake_security(dir.path(),
        &format!("printf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' '{SECRET}'", argv.display()));
    let key = read_with(&program, &item()).expect("key");
    assert_eq!(std::fs::read_to_string(&argv).expect("argv"),
        "find-generic-password\n-s\naid-anthropic-api\n-a\nclaude-api-meet\n-w\n");
    assert_eq!(read_env(key), format!("{SECRET}\n"));
}

#[test]
fn missing_item_fails_without_echoing_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = fake_security(dir.path(),
        &format!("printf '%s' '{SECRET}'\necho 'item could not be found' >&2\nexit 44"));
    let error = format!("{:#}", read_with(&program, &item()).err().expect("refused"));
    assert!(error.contains("aid-anthropic-api/claude-api-meet"), "{error}");
    assert!(error.contains("exit 44"), "{error}");
    assert!(!error.contains(SECRET), "{error}");
}

#[test]
fn rejects_empty_and_multi_line_values_without_echoing_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    for body in ["printf ''".to_string(), format!("printf '%s\\n%s\\n' '{SECRET}' second")] {
        let program = fake_security(dir.path(), &body);
        let error = format!("{:#}", read_with(&program, &item()).err().expect("refused"));
        assert!(error.contains("single-line key"), "{error}");
        assert!(!error.contains(SECRET), "{error}");
    }
}

#[test]
fn applies_only_to_the_child_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = fake_security(dir.path(), &format!("printf '%s\\n' '{SECRET}'"));
    let key = read_with(&program, &item()).expect("key");
    assert_eq!(read_env(key), format!("{SECRET}\n"));
    assert!(std::env::var_os("AID_TEST_KEY").is_none());
}

#[test]
fn production_reader_uses_the_absolute_security_path() {
    assert_eq!(SECURITY, "/usr/bin/security");
}
