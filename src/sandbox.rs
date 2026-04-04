// Container sandbox helpers for agent process execution.
// Exports command wrapping, availability checks, and container cleanup.

use std::process::{Command, Stdio};

use crate::types::AgentKind;

const CONTAINER_BIN: &str = "container";
const SANDBOX_IMAGE: &str = "aid-sandbox:latest";

pub fn can_sandbox(agent_kind: AgentKind) -> bool {
    !matches!(
        agent_kind,
        AgentKind::OpenCode
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
        wrapped.arg("-v").arg(format!("{dir}:{dir}"));
        wrapped.arg("-w").arg(dir);
        wrapped.current_dir(dir);
    }
    // Forward env vars explicitly set on the command
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            wrapped.arg("-e").arg(format!(
                "{}={}",
                key.to_string_lossy(),
                value.to_string_lossy()
            ));
        }
    }
    // Forward agent-specific API keys from host environment (inherit mode)
    for key in agent_env_keys(agent_kind) {
        if std::env::var_os(key).is_some() {
            wrapped.arg("-e").arg(*key);
        }
    }
    // Mount agent config directories (auth, settings) from host
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
    wrapped.arg("-e").arg("HOME=/root");
    wrapped.arg(SANDBOX_IMAGE);
    wrapped.arg(cmd.get_program());
    wrapped.args(cmd.get_args());
    wrapped
}

fn agent_env_keys(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Codex => &["OPENAI_API_KEY"],
        AgentKind::Gemini => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
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

#[cfg(test)]
mod tests {
    use super::{can_sandbox, wrap_command};
    use crate::types::AgentKind;
    use std::{
        ffi::OsString,
        fs,
        process::Command,
        sync::{Mutex, OnceLock},
    };
    use tempfile::tempdir;

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
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
}
