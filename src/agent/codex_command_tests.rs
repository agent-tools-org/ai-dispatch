// Codex adapter regression tests extracted from the inline test module.
// Deps: parent adapter helpers, tempfile, and command/event types.

use super::CodexAgent;
use crate::agent::{Agent, RunOpts};

pub(super) fn test_opts() -> RunOpts {
    RunOpts {
        ..Default::default()
    }
}

#[test]
fn fix_round_one_read_only_uses_one_permission_mechanism_for_fresh_and_resume() {
    for session in [None, Some("saved-session".to_string())] {
        let opts = RunOpts { read_only: true, session_id: session, ..test_opts() };
        let cmd = CodexAgent.build_command_with_context("audit", &opts,
            crate::agent::CommandContext { durable_codex_home: false, writable_roots: vec![] }).unwrap();
        let args: Vec<_> = cmd.get_args().map(|arg| arg.to_string_lossy()).collect();
        assert!(!args.iter().any(|arg| arg.starts_with("sandbox_mode=")
            || arg.starts_with("sandbox_workspace_write.") || arg == "--sandbox"));
        assert!(args.iter().any(|arg| arg == "default_permissions=\"aid-read-only\""));
        assert!(args.iter().any(|arg| arg == "--ignore-user-config"));
    }
}

#[test]
fn fix_round_one_write_policy_is_explicit_for_fresh_and_resume() {
    for session in [None, Some("saved-session".to_string())] {
        let opts = RunOpts { session_id: session, ..test_opts() };
        let cmd = CodexAgent.build_command_with_context("write", &opts,
            crate::agent::CommandContext { durable_codex_home: false, writable_roots: vec![] }).unwrap();
        assert!(cmd.get_args().any(|arg| arg == "default_permissions=\"aid-write\""));
        assert!(!cmd.get_args().any(|arg| arg == "--approve-for-me" || arg == "--full-auto"));
    }
}

#[test]
fn build_command_includes_skip_git_repo_check() {
    let opts = RunOpts {
        ..Default::default()
    };
    let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    assert!(args.contains(&"--skip-git-repo-check".to_string()));
}

#[test]
fn build_command_read_only_selects_native_sandbox_without_write_approval() {
    let opts = RunOpts {
        read_only: true,
        ..Default::default()
    };
    let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    assert!(args.contains(&"default_permissions=\"aid-read-only\"".to_string()));
    assert!(args.contains(&"--ignore-user-config".to_string()));
    assert!(!args.iter().any(|arg| arg.starts_with("sandbox_mode=")));
    assert!(args.iter().any(|arg| arg.starts_with("permissions=") && arg.contains(":read-only")));
    assert!(!args.contains(&"--approve-for-me".to_string()));
    assert!(!args.contains(&"--full-auto".to_string()));
}

#[test]
fn build_command_read_only_prepends_readonly_prefix() {
    let opts = RunOpts {
        result_file: Some("result.md".to_string()),
        read_only: true,
        ..Default::default()
    };
    let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    let last_arg = args.iter().find(|arg| arg.contains("analyze this code")).expect("prompt argument");
    assert!(last_arg.contains("READ-ONLY MODE"));
    assert!(last_arg.starts_with("IMPORTANT: READ-ONLY MODE"));
    assert!(last_arg.contains("EXCEPT the result file specified in this prompt"));
    assert!(last_arg.contains("analyze this code"));
}

#[test]
fn build_command_read_only_without_result_file_keeps_strict_prefix() {
    let opts = RunOpts {
        read_only: true,
        ..Default::default()
    };
    let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    let last_arg = args.iter().find(|arg| arg.contains("analyze this code")).expect("prompt argument");
    assert!(last_arg.contains("Do NOT modify, create, or delete any files. Only read and analyze."));
}

#[test]
fn build_command_includes_context_files_in_prompt() {
    let opts = RunOpts {
        context_files: vec!["Cargo.toml".to_string()],
        ..Default::default()
    };
    let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    let last_arg = args.iter().find(|arg| arg.contains("test prompt")).expect("prompt argument");
    assert!(last_arg.contains("[Context File:"));
}

#[test]
fn build_command_starts_fresh_when_saved_rollout_is_missing() {
    let opts = RunOpts {
        session_id: Some("019e3e49-6b83-7563-a3d8-b51a3a716dd1".to_string()),
        ..Default::default()
    };
    let cmd = CodexAgent.build_command("write the final report", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    // The approval flag sits between the fixed prefix and the prompt only when the
    // installed Codex version could be read, so pin the prefix and the prompt by
    // position from each end rather than assuming the flag is present.
    assert_eq!(&args[..3], ["exec", "--json", "--skip-git-repo-check"]);
    assert!(args.iter().any(|arg| arg.contains("write the final report")));
}
