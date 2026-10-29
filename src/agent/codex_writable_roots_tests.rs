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
    fs::create_dir_all(&cargo_target).unwrap();
    let roots = vec![metadata.clone(), common.clone(), cargo_target.canonicalize().unwrap()];
    let opts = RunOpts {
        dir: Some(worktree.to_string_lossy().into_owned()),
        sandbox: true,
        ..Default::default()
    };
    let cmd = CodexAgent
        .build_command_with_context(
            "test prompt",
            &opts,
            CommandContext {
                durable_codex_home: false,
                writable_roots: roots,
            },
        )
        .unwrap();
    let args: Vec<String> = crate::support::test_command::args(&cmd);
    let config = args.iter().find(|arg| arg.starts_with("permissions=")).unwrap();
    let parsed: toml::Value = config.parse().unwrap();
    let roots = parsed["permissions"]["aid-write"]["filesystem"].as_table().unwrap();
    assert_eq!(roots.len(), 3);
    for path in [metadata, common, cargo_target.canonicalize().unwrap()] {
        assert_eq!(roots[path.to_str().unwrap()].as_str(), Some("write"));
    }
    assert!(args.contains(&"default_permissions=\"aid-write\"".to_string()));

}

#[test]
fn read_only_command_grants_only_supplied_task_and_scratch_directories_on_resume() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let task = temp.path().join("task");
    let scratch = temp.path().join("scratch");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir(&task).unwrap();
    fs::create_dir(&scratch).unwrap();
    let opts = RunOpts {
        dir: Some(repo.to_string_lossy().into_owned()),
        read_only: true,
        session_id: Some("saved-session".into()),
        ..crate::agent::RunOpts::default()
    };
    let cmd = CodexAgent.build_command_with_context("audit", &opts, CommandContext {
        durable_codex_home: false,
        writable_roots: vec![task.clone(), scratch.clone()],
    }).unwrap();
    let args: Vec<_> = crate::support::test_command::args(&cmd);
    assert_eq!(&args[..2], ["exec", "resume"]);
    let config = args.iter().find(|arg| arg.starts_with("permissions=")).unwrap();
    let parsed: toml::Value = config.parse().unwrap();
    let profile = &parsed["permissions"]["aid-read-only"];
    assert_eq!(profile["extends"].as_str(), Some(":read-only"));
    let roots = profile["filesystem"].as_table().unwrap();
    for path in [task, scratch] {
        assert_eq!(roots[path.to_str().unwrap()].as_str(), Some("write"));
    }
    assert!(!roots.contains_key(repo.to_str().unwrap()));
    assert!(!roots.contains_key(repo.join(".git").to_str().unwrap()));
    assert!(!args.iter().any(|arg| arg.starts_with("sandbox_workspace_write.")));
    assert!(args.contains(&"approval_policy=\"never\"".to_string()));
}
