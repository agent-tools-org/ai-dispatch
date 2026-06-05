// Unit tests for container sandbox command wrapping.
// Covers mount/env behavior for regular checkouts and linked git worktrees.

use crate::sandbox::{can_sandbox, wrap_command};
use crate::types::AgentKind;
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::Command,
    sync::{Mutex, OnceLock},
};
use tempfile::tempdir;

fn args(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

fn self_mount(path: &Path) -> String {
    let path = path.to_string_lossy();
    format!("{path}:{path}")
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct HomeGuard(Option<OsString>);

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(home) => unsafe {
                std::env::set_var("HOME", home);
            },
            None => unsafe {
                std::env::remove_var("HOME");
            },
        }
    }
}

fn with_home<F>(dirs: &[&str], test: F)
where
    F: FnOnce(),
{
    let _guard = env_lock().lock().expect("env lock poisoned");
    let temp = tempdir().expect("tempdir");
    for dir in dirs {
        fs::create_dir_all(temp.path().join(dir)).expect("create home subdir");
    }
    let original_home = std::env::var_os("HOME");
    let _home_guard = HomeGuard(original_home);
    unsafe {
        std::env::set_var("HOME", temp.path());
    }
    test();
}

#[test]
fn cannot_sandbox_native_agents() {
    assert!(!can_sandbox(AgentKind::OpenCode));
    assert!(!can_sandbox(AgentKind::Copilot));
    assert!(!can_sandbox(AgentKind::Cursor));
    assert!(!can_sandbox(AgentKind::Droid));
    assert!(!can_sandbox(AgentKind::Oz));
    assert!(!can_sandbox(AgentKind::Claude));
    assert!(!can_sandbox(AgentKind::Custom));
    assert!(can_sandbox(AgentKind::Codex));
}

#[test]
fn wrap_command_builds_container_run() {
    let mut cmd = Command::new("codex");
    cmd.args(["exec", "ship it"]);

    let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
    let wrapped_args = args(&wrapped);

    assert_eq!(wrapped.get_program().to_string_lossy(), "container");
    assert!(wrapped_args.iter().any(|arg| arg == "run"));
    assert!(wrapped_args.iter().any(|arg| arg == "--rm"));
    assert!(wrapped_args.iter().any(|arg| arg == "--init"));
    assert!(wrapped_args.iter().any(|arg| arg == "aid-sandbox:latest"));
    assert_eq!(wrapped_args[wrapped_args.len() - 3], "codex");
    assert_eq!(wrapped_args[wrapped_args.len() - 2], "exec");
    assert_eq!(wrapped_args[wrapped_args.len() - 1], "ship it");
}

#[test]
fn wrap_command_forwards_env_vars() {
    let mut cmd = Command::new("codex");
    cmd.env("OPENAI_API_KEY", "test-key");

    let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
    let wrapped_args = args(&wrapped);

    assert!(wrapped_args
        .windows(2)
        .any(|pair| pair == ["-e", "OPENAI_API_KEY=test-key"]));
}

#[test]
fn wrap_command_mounts_project_dir() {
    with_home(&[".aid"], || {
        let mut cmd = Command::new("codex");
        cmd.current_dir("/tmp/project");

        let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
        let wrapped_args = args(&wrapped);

        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-v", "/tmp/project:/tmp/project"]));
        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-w", "/tmp/project"]));
        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair[0] == "-v" && pair[1].ends_with(":/root/.aid")));
    });
}

#[test]
fn wrap_command_mounts_linked_worktree_gitdirs() {
    let temp = tempdir().expect("tempdir");
    let worktree = temp.path().join("worktree");
    let common = temp.path().join("common/.git");
    let gitdir = common.join("worktrees/feature");
    fs::create_dir_all(&worktree).expect("create worktree");
    fs::create_dir_all(&gitdir).expect("create gitdir");
    fs::write(
        worktree.join(".git"),
        "gitdir: ../common/.git/worktrees/feature\n",
    )
    .expect("write gitfile");
    fs::write(gitdir.join("commondir"), "../..\n").expect("write commondir");
    let gitdir = gitdir.canonicalize().expect("canonical gitdir");
    let common = common.canonicalize().expect("canonical common");
    let mut cmd = Command::new("codex");
    cmd.current_dir(&worktree);

    let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
    let wrapped_args = args(&wrapped);
    let gitdir_mount = self_mount(&gitdir);
    let common_mount = self_mount(&common);

    assert!(!wrapped_args
        .windows(2)
        .any(|pair| pair[0] == "-v" && pair[1] == gitdir_mount));
    assert!(wrapped_args
        .windows(2)
        .any(|pair| pair[0] == "-v" && pair[1] == common_mount));
}

#[test]
fn wrap_command_mounts_worktree_gitdir_without_commondir() {
    let temp = tempdir().expect("tempdir");
    let worktree = temp.path().join("worktree");
    let gitdir = temp.path().join("gitdirs/feature");
    fs::create_dir_all(&worktree).expect("create worktree");
    fs::create_dir_all(&gitdir).expect("create gitdir");
    fs::write(worktree.join(".git"), "gitdir: ../gitdirs/feature\n").expect("write gitfile");
    let gitdir = gitdir.canonicalize().expect("canonical gitdir");
    let mut cmd = Command::new("codex");
    cmd.current_dir(&worktree);

    let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
    let wrapped_args = args(&wrapped);
    let gitdir_mount = self_mount(&gitdir);

    assert!(wrapped_args
        .windows(2)
        .any(|pair| pair[0] == "-v" && pair[1] == gitdir_mount));
}

#[test]
fn wrap_command_skips_git_mounts_for_regular_checkout() {
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let gitdir = repo.join(".git");
    fs::create_dir_all(&gitdir).expect("create gitdir");
    let mut cmd = Command::new("codex");
    cmd.current_dir(&repo);

    let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
    let wrapped_args = args(&wrapped);
    let repo_mount = self_mount(&repo);
    let gitdir_mount = self_mount(&gitdir);

    assert!(wrapped_args
        .windows(2)
        .any(|pair| pair[0] == "-v" && pair[1] == repo_mount));
    assert!(!wrapped_args
        .windows(2)
        .any(|pair| pair[0] == "-v" && pair[1] == gitdir_mount));
}

#[test]
fn wrap_command_mounts_aid_home() {
    with_home(&[".aid"], || {
        let cmd = Command::new("codex");

        let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, false);
        let wrapped_args = args(&wrapped);

        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair[0] == "-v" && pair[1].ends_with(":/root/.aid")));
        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-e", "AID_HOME=/root/.aid"]));
    });
}

#[test]
fn wrap_command_readonly_adds_flag() {
    let cmd = Command::new("codex");

    let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex, true);
    let wrapped_args = args(&wrapped);

    assert!(wrapped_args.iter().any(|arg| arg == "--read-only"));
    assert!(wrapped_args
        .windows(2)
        .any(|pair| pair == ["--tmpfs", "/tmp"]));
}
