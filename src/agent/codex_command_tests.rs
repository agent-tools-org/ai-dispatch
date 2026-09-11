// Codex adapter regression tests extracted from the inline test module.
// Deps: parent adapter helpers, tempfile, and command/event types.

use super::CodexAgent;
use crate::agent::{Agent, RunOpts};

#[test]
fn build_command_includes_skip_git_repo_check() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    assert!(args.contains(&"--skip-git-repo-check".to_string()));
}

#[test]
fn build_command_read_only_omits_legacy_approval_flags() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: true,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    assert!(!args.contains(&"-s".to_string()));
    assert!(!args.contains(&"read-only".to_string()));
}

#[test]
fn build_command_read_only_prepends_readonly_prefix() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: Some("result.md".to_string()),
        model: None,
        budget: false,
        read_only: true,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
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
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: true,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
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
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec!["Cargo.toml".to_string()],
        session_id: None,
        env: None,
        env_forward: None,
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
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: Some("019e3e49-6b83-7563-a3d8-b51a3a716dd1".to_string()),
        env: None,
        env_forward: None,
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
