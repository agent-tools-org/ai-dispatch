// Codex adapter regression tests extracted from the inline test module.
// Deps: parent adapter helpers, tempfile, and command/event types.

use super::CodexAgent;
use crate::agent::{Agent, RunOpts};
use std::fs;
use tempfile::tempdir;

#[test]
fn build_command_adds_worktree_metadata_to_writable_roots() {
    let temp = tempdir().unwrap();
    let worktree = temp.path().join("worktree");
    let common = temp.path().join("common/.git");
    let metadata = common.join("worktrees/bar");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&common).unwrap();
    fs::create_dir_all(&metadata).unwrap();
    fs::write(worktree.join(".git"), "gitdir: ../common/.git/worktrees/bar\n").unwrap();
    fs::write(metadata.join("commondir"), "../..\n").unwrap();
    let metadata = metadata.canonicalize().unwrap();
    let common = common.canonicalize().unwrap();
    let opts = RunOpts {
        dir: Some(worktree.to_string_lossy().to_string()),
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

    assert!(args.contains(&"-c".to_string()));
    let expected = format!(
        "sandbox_workspace_write.writable_roots={}",
        toml::Value::Array(vec![
            toml::Value::String(metadata.to_string_lossy().to_string()),
            toml::Value::String(common.to_string_lossy().to_string()),
        ])
    );
    assert!(args.contains(&expected));
}

#[test]
fn build_command_falls_back_when_commondir_missing() {
    let temp = tempdir().unwrap();
    let worktree = temp.path().join("worktree");
    let metadata = temp.path().join("foo/.git/worktrees/bar");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&metadata).unwrap();
    fs::write(worktree.join(".git"), "gitdir: ../foo/.git/worktrees/bar\n").unwrap();
    let metadata = metadata.canonicalize().unwrap();
    let opts = RunOpts {
        dir: Some(worktree.to_string_lossy().to_string()),
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

    assert!(args.contains(&"-c".to_string()));
    let expected = format!(
        "sandbox_workspace_write.writable_roots={}",
        toml::Value::Array(vec![toml::Value::String(metadata.to_string_lossy().to_string())])
    );
    assert!(args.contains(&expected));
}

#[test]
fn build_command_grants_writable_roots_for_regular_repo() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let opts = RunOpts {
        dir: Some(repo.to_string_lossy().to_string()),
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

    assert!(args.iter().any(|arg| {
        arg.starts_with("sandbox_workspace_write.writable_roots=")
    }));
}

#[test]
fn build_command_handles_missing_gitfile_gracefully() {
    let temp = tempdir().unwrap();
    let opts = RunOpts {
        dir: Some(temp.path().to_string_lossy().to_string()),
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

    assert!(!args.iter().any(|arg| {
        arg.starts_with("sandbox_workspace_write.writable_roots=")
    }));
}
