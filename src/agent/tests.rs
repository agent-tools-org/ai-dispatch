// Integration-style tests for agent env helpers and subprocess markers.
// Exports: none (test module). Deps: crate::agent, crate::paths, tempfile.

use super::{apply_run_env, is_rust_project, set_git_ceiling, shared_target_dir, target_dir_for_worktree, RunOpts};
use crate::paths::AidHomeGuard;
use crate::test_subprocess;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[test]
fn set_git_ceiling_uses_parent_dir() {
    let mut cmd = Command::new("echo");
    set_git_ceiling(&mut cmd, "/tmp/cloned-repo");
    let envs: Vec<_> = cmd.get_envs().collect();
    let ceiling = envs
        .iter()
        .find(|(k, _)| *k == "GIT_CEILING_DIRECTORIES")
        .and_then(|(_, v)| v.as_ref())
        .map(|v| v.to_string_lossy().to_string());
    assert_eq!(ceiling.as_deref(), Some("/tmp"));
}

#[test]
fn detects_rust_project_in_current_dir() {
    let _permit = test_subprocess::acquire();
    let temp_dir = TempDir::new().unwrap();

    std::fs::write(
        temp_dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();

    let output = run_helper(
        "agent::tests::reports_is_rust_project_for_subprocess",
        Some(temp_dir.path()),
        &[],
    );
    assert_eq!(extract_marker(&output, "IS_RUST_PROJECT="), "true");
}

#[test]
fn detects_rust_project_from_explicit_dir() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path().to_string_lossy().into_owned();

    std::fs::write(
        temp_dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();

    assert!(is_rust_project(Some(&dir)));
}

#[test]
fn returns_false_when_manifest_is_missing() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path().to_string_lossy().into_owned();

    assert!(!is_rust_project(Some(&dir)));
}

#[test]
fn shared_target_dir_prefers_explicit_env_var() {
    let _permit = test_subprocess::acquire();
    let temp_dir = TempDir::new().unwrap();
    let expected = temp_dir.path().join("shared-target");
    let output = run_helper(
        "agent::tests::reports_shared_target_dir_for_subprocess",
        None,
        &[("CARGO_TARGET_DIR", Some(expected.as_os_str()))],
    );
    assert_eq!(
        extract_marker(&output, "SHARED_TARGET_DIR="),
        expected.to_string_lossy()
    );
}

#[test]
fn shared_target_dir_defaults_under_aid_home() {
    let _permit = test_subprocess::acquire();
    let temp_dir = TempDir::new().unwrap();
    let aid_home = temp_dir.path().join("aid-home");
    let expected = aid_home.join("cargo-target");
    let output = run_helper(
        "agent::tests::reports_shared_target_dir_for_subprocess",
        None,
        &[
            ("CARGO_TARGET_DIR", None),
            ("AID_HOME", Some(aid_home.as_os_str())),
        ],
    );
    assert_eq!(
        extract_marker(&output, "SHARED_TARGET_DIR="),
        expected.to_string_lossy()
    );
}

#[test]
fn shared_target_dir_defaults_to_home_aid_path() {
    let _permit = test_subprocess::acquire();
    let temp_dir = TempDir::new().unwrap();
    let home_dir = temp_dir.path().join("home");
    let expected = PathBuf::from(&home_dir).join(".aid").join("cargo-target");
    let output = run_helper(
        "agent::tests::reports_shared_target_dir_for_subprocess",
        None,
        &[
            ("CARGO_TARGET_DIR", None),
            ("AID_HOME", None),
            ("HOME", Some(home_dir.as_os_str())),
        ],
    );
    assert_eq!(
        extract_marker(&output, "SHARED_TARGET_DIR="),
        expected.to_string_lossy()
    );
}

#[test]
#[ignore]
fn reports_is_rust_project_for_subprocess() {
    let _permit = test_subprocess::acquire();
    println!("IS_RUST_PROJECT={}", is_rust_project(None));
}

#[test]
#[ignore]
fn reports_shared_target_dir_for_subprocess() {
    let _permit = test_subprocess::acquire();
    println!(
        "SHARED_TARGET_DIR={}",
        shared_target_dir().unwrap_or_default()
    );
}

#[test]
fn target_dir_for_worktree_isolates_branches() {
    let base = shared_target_dir().unwrap();
    let isolated = target_dir_for_worktree(Some("feat/my-feature")).unwrap();
    assert_eq!(isolated, format!("{base}/feat-my-feature"));
    let shared = target_dir_for_worktree(None).unwrap();
    assert_eq!(shared, base);
}

#[test]
fn apply_run_env_sets_explicit_vars_on_command() {
    let mut cmd = Command::new("echo");
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: Some([("APP_MODE".to_string(), "test".to_string())].into_iter().collect()),
        env_forward: None,
    };

    apply_run_env(&mut cmd, &opts);

    let envs: Vec<_> = cmd.get_envs().collect();
    let mode = envs
        .iter()
        .find(|(key, _)| *key == "APP_MODE")
        .and_then(|(_, value)| value.as_ref())
        .map(|value| value.to_string_lossy().to_string());
    assert_eq!(mode.as_deref(), Some("test"));
}

#[test]
fn apply_run_env_sets_aid_home_on_command() {
    let temp_dir = TempDir::new().unwrap();
    let aid_home = temp_dir.path().join("aid-home");
    let _guard = AidHomeGuard::set(&aid_home);
    let mut cmd = Command::new("echo");
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: Some(HashMap::new()),
        env_forward: Some(vec![]),
    };

    apply_run_env(&mut cmd, &opts);

    let envs: Vec<_> = cmd.get_envs().collect();
    let aid_home_env = envs
        .iter()
        .find(|(key, _)| *key == "AID_HOME")
        .and_then(|(_, value)| value.as_ref())
        .map(|value| value.to_string_lossy().to_string());
    assert_eq!(aid_home_env.as_deref(), Some(aid_home.to_string_lossy().as_ref()));
}

#[test]
fn apply_run_env_forwards_parent_vars() {
    let _permit = test_subprocess::acquire();
    let output = run_helper(
        "agent::tests::reports_forwarded_env_for_subprocess",
        None,
        &[("AID_TEST_FORWARDED_ENV", Some(OsStr::new("forwarded-value")))],
    );
    assert_eq!(
        extract_marker(&output, "FORWARDED_ENV="),
        "forwarded-value"
    );
}

#[test]
#[ignore]
fn reports_forwarded_env_for_subprocess() {
    let _permit = test_subprocess::acquire();
    let mut cmd = Command::new("echo");
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: Some(vec!["AID_TEST_FORWARDED_ENV".to_string()]),
    };
    apply_run_env(&mut cmd, &opts);
    let envs: Vec<_> = cmd.get_envs().collect();
    let forwarded = envs
        .iter()
        .find(|(key, _)| *key == "AID_TEST_FORWARDED_ENV")
        .and_then(|(_, value)| value.as_ref())
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    println!("FORWARDED_ENV={forwarded}");
}

fn run_helper(
    test_name: &str,
    current_dir: Option<&Path>,
    env_vars: &[(&str, Option<&OsStr>)],
) -> String {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test_name, "--ignored", "--nocapture"]);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    for (name, value) in env_vars {
        if let Some(value) = value {
            cmd.env(name, value);
        } else {
            cmd.env_remove(name);
        }
    }

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "helper test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap()
}

fn extract_marker<'a>(output: &'a str, prefix: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing marker {prefix} in output: {output}"))
}
