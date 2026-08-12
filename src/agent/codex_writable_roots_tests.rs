// Codex writable-root regression tests for worktree cargo targets.
// Exports: none; tests the parent codex adapter's command construction.
// Deps: CodexAgent, CommandContext, RunOpts, tempfile.

use super::CodexAgent;
use crate::agent::{Agent, CommandContext, RunOpts};
use std::fs;
use tempfile::tempdir;

#[test]
fn build_command_adds_effective_target_without_granting_parent() {
    let temp = tempdir().unwrap();
    let worktree = temp.path().join("worktree");
    let common = temp.path().join("common/.git");
    let metadata = common.join("worktrees/bar");
    let cargo_target = temp.path().join("cargo-target/feat-bar");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&common).unwrap();
    fs::create_dir_all(&metadata).unwrap();
    fs::write(worktree.join(".git"), "gitdir: ../common/.git/worktrees/bar\n").unwrap();
    fs::write(metadata.join("commondir"), "../..\n").unwrap();
    let metadata = metadata.canonicalize().unwrap();
    let common = common.canonicalize().unwrap();
    let opts = RunOpts {
        dir: Some(worktree.to_string_lossy().into_owned()),
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: true,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = CodexAgent
        .build_command_with_context(
            "test prompt",
            &opts,
            CommandContext {
                durable_codex_home: false,
                cargo_target_dir: Some(cargo_target.to_string_lossy().into_owned()),
            },
        )
        .unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let expected = format!(
        "sandbox_workspace_write.writable_roots={}",
        toml::Value::Array(vec![
            toml::Value::String(metadata.to_string_lossy().into_owned()),
            toml::Value::String(common.to_string_lossy().into_owned()),
            toml::Value::String(cargo_target.to_string_lossy().into_owned()),
        ])
    );

    assert!(args.contains(&expected));
}
