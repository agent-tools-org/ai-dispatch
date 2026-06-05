// Container sandbox helpers for agent process execution.
// Exports command wrapping, availability checks, and container cleanup.

use std::process::{Command, Stdio};

use crate::types::AgentKind;
use crate::worktree_layout::{read_commondir, resolve_worktree_gitdir};

const CONTAINER_BIN: &str = "container";
const SANDBOX_IMAGE: &str = "aid-sandbox:latest";

pub fn can_sandbox(agent_kind: AgentKind) -> bool {
    !matches!(
        agent_kind,
        AgentKind::OpenCode
            | AgentKind::Copilot
            | AgentKind::Cursor
            | AgentKind::Droid
            | AgentKind::Oz
            | AgentKind::Claude
            | AgentKind::Custom
    )
}

pub fn is_available() -> bool {
    Command::new(CONTAINER_BIN)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn wrap_command(
    cmd: &Command,
    task_id: &str,
    agent_kind: AgentKind,
    read_only: bool,
) -> Command {
    let cwd = cmd
        .get_current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        });
    let mut wrapped = Command::new(CONTAINER_BIN);
    wrapped
        .arg("run")
        .arg("--rm")
        .arg("--init")
        .arg("--name")
        .arg(format!("aid-{task_id}"));
    if read_only {
        wrapped.arg("--read-only");
        wrapped.arg("--tmpfs").arg("/tmp");
    }
    if let Some(dir) = cwd.as_deref() {
        mount_workdir(&mut wrapped, dir);
    }
    forward_command_envs(&mut wrapped, cmd);
    forward_agent_envs(&mut wrapped, agent_kind);
    mount_agent_home(&mut wrapped, agent_kind);
    wrapped.arg("-e").arg("HOME=/root");
    wrapped.arg(SANDBOX_IMAGE);
    wrapped.arg(cmd.get_program());
    wrapped.args(cmd.get_args());
    wrapped
}

fn mount_workdir(wrapped: &mut Command, dir: &str) {
    wrapped.arg("-v").arg(format!("{dir}:{dir}"));
    for (host, container) in worktree_git_mounts(dir) {
        wrapped.arg("-v").arg(format!("{host}:{container}"));
    }
    wrapped.arg("-w").arg(dir);
    wrapped.current_dir(dir);
}

fn forward_command_envs(wrapped: &mut Command, cmd: &Command) {
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            wrapped.arg("-e").arg(format!(
                "{}={}",
                key.to_string_lossy(),
                value.to_string_lossy()
            ));
        }
    }
}

fn forward_agent_envs(wrapped: &mut Command, agent_kind: AgentKind) {
    for key in agent_env_keys(agent_kind) {
        if std::env::var_os(key).is_some() {
            wrapped.arg("-e").arg(*key);
        }
    }
}

fn mount_agent_home(wrapped: &mut Command, agent_kind: AgentKind) {
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::Path::new(&home);
        for subdir in agent_config_dirs(agent_kind) {
            let host_path = home.join(subdir);
            if host_path.exists() {
                let container_path = std::path::Path::new("/root").join(subdir);
                wrapped.arg("-v").arg(format!(
                    "{}:{}",
                    host_path.display(),
                    container_path.display()
                ));
            }
        }
        let aid_home = home.join(".aid");
        if aid_home.exists() {
            wrapped
                .arg("-v")
                .arg(format!("{}:/root/.aid", aid_home.display()))
                .arg("-e")
                .arg("AID_HOME=/root/.aid");
        }
    }
}

fn worktree_git_mounts(cwd: &str) -> Vec<(String, String)> {
    let cwd_path = std::path::Path::new(cwd);
    let cwd_mount = std::fs::canonicalize(cwd_path).unwrap_or_else(|_| cwd_path.to_path_buf());
    let Some(gitdir) = resolve_worktree_gitdir(cwd_path) else {
        return Vec::new();
    };
    let mut mounts = Vec::new();
    if let Some(commondir) = read_commondir(&gitdir) {
        push_git_mount(&mut mounts, &cwd_mount, &commondir);
        if !gitdir.starts_with(&commondir) {
            push_git_mount(&mut mounts, &cwd_mount, &gitdir);
        }
    } else {
        push_git_mount(&mut mounts, &cwd_mount, &gitdir);
    }
    mounts
}

fn push_git_mount(
    mounts: &mut Vec<(String, String)>,
    cwd: &std::path::Path,
    path: &std::path::Path,
) {
    if path.starts_with(cwd) {
        return;
    }
    let Some(mount) = path.to_str().map(ToOwned::to_owned) else {
        return;
    };
    if mounts.iter().any(|(host, _)| host == &mount) {
        return;
    }
    mounts.push((mount.clone(), mount));
}

fn agent_env_keys(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Codex => &["OPENAI_API_KEY"],
        AgentKind::Gemini => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        AgentKind::Qwen => &[],
        AgentKind::Copilot => &[],
        AgentKind::Kilo => &["KILO_API_KEY", "OPENAI_API_KEY"],
        AgentKind::Codebuff => &["CODEBUFF_API_KEY", "ANTHROPIC_API_KEY"],
        AgentKind::Claude => &["ANTHROPIC_API_KEY"],
        _ => &[],
    }
}

/// Config directory paths relative to $HOME that each agent needs for auth/settings.
fn agent_config_dirs(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Codex => &[".codex"],
        AgentKind::Gemini => &[".gemini"],
        AgentKind::Qwen => &[".qwen"],
        AgentKind::Copilot => &[".copilot"],
        AgentKind::Kilo => &[".kilo"],
        AgentKind::Codebuff => &[".codebuff"],
        AgentKind::Claude => &[".claude"],
        _ => &[],
    }
}

pub fn kill_container(task_id: &str) {
    let name = format!("aid-{task_id}");
    let _ = Command::new(CONTAINER_BIN)
        .args(["kill", &name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new(CONTAINER_BIN)
        .args(["rm", &name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
