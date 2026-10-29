// Shared command inspection for adapter and runtime tests.
// Exports: args and env; preserves lossy UTF-8 conversion and explicitly set env values.
// Deps: std::process::Command.

use std::process::Command;

pub(crate) fn args(command: &Command) -> Vec<String> {
    command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect()
}

pub(crate) fn env(command: &Command, name: &str) -> Option<String> {
    command.get_envs()
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| value)
        .map(|value| value.to_string_lossy().into_owned())
}

/// Spawn the shell fixture used by watcher tests with both output streams captured.
pub(crate) fn piped_shell(dir: &std::path::Path, script: &str) -> tokio::process::Child {
    tokio::process::Command::new("sh")
        .current_dir(dir)
        .args(["-c", script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("shell fixture")
}
