// Container sandbox helpers for agent process execution.
// Exports command wrapping, availability checks, and container cleanup.

use std::process::{Command, Stdio};

use crate::types::AgentKind;

const CONTAINER_BIN: &str = "container";
const SANDBOX_IMAGE: &str = "ubuntu:24.04";

pub fn can_sandbox(agent_kind: AgentKind) -> bool {
    !matches!(
        agent_kind,
        AgentKind::OpenCode
            | AgentKind::Cursor
            | AgentKind::Droid
            | AgentKind::Oz
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

pub fn wrap_command(cmd: &Command, task_id: &str, _agent_kind: AgentKind) -> Command {
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
    if let Some(dir) = cwd.as_deref() {
        wrapped.arg("-v").arg(format!("{dir}:{dir}"));
        wrapped.arg("-w").arg(dir);
        wrapped.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            wrapped.arg("-e").arg(format!(
                "{}={}",
                key.to_string_lossy(),
                value.to_string_lossy()
            ));
        }
    }
    wrapped.arg(SANDBOX_IMAGE);
    wrapped.arg(cmd.get_program());
    wrapped.args(cmd.get_args());
    wrapped
}

pub fn kill_container(task_id: &str) {
    let _ = Command::new(CONTAINER_BIN)
        .args(["kill", &format!("aid-{task_id}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::{can_sandbox, wrap_command};
    use crate::types::AgentKind;
    use std::process::Command;

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn cannot_sandbox_native_agents() {
        assert!(!can_sandbox(AgentKind::OpenCode));
        assert!(!can_sandbox(AgentKind::Cursor));
        assert!(!can_sandbox(AgentKind::Droid));
        assert!(!can_sandbox(AgentKind::Oz));
        assert!(!can_sandbox(AgentKind::Custom));
        assert!(can_sandbox(AgentKind::Codex));
    }

    #[test]
    fn wrap_command_builds_container_run() {
        let mut cmd = Command::new("codex");
        cmd.args(["exec", "ship it"]);

        let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex);
        let wrapped_args = args(&wrapped);

        assert_eq!(wrapped.get_program().to_string_lossy(), "container");
        assert!(wrapped_args.iter().any(|arg| arg == "run"));
        assert!(wrapped_args.iter().any(|arg| arg == "--rm"));
        assert!(wrapped_args.iter().any(|arg| arg == "--init"));
        assert!(wrapped_args.iter().any(|arg| arg == "ubuntu:24.04"));
        assert_eq!(wrapped_args[wrapped_args.len() - 3], "codex");
        assert_eq!(wrapped_args[wrapped_args.len() - 2], "exec");
        assert_eq!(wrapped_args[wrapped_args.len() - 1], "ship it");
    }

    #[test]
    fn wrap_command_forwards_env_vars() {
        let mut cmd = Command::new("codex");
        cmd.env("OPENAI_API_KEY", "test-key");

        let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex);
        let wrapped_args = args(&wrapped);

        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-e", "OPENAI_API_KEY=test-key"]));
    }

    #[test]
    fn wrap_command_mounts_cwd() {
        let mut cmd = Command::new("codex");
        cmd.current_dir("/tmp/project");

        let wrapped = wrap_command(&cmd, "t-abcd", AgentKind::Codex);
        let wrapped_args = args(&wrapped);

        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-v", "/tmp/project:/tmp/project"]));
        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-w", "/tmp/project"]));
    }
}
