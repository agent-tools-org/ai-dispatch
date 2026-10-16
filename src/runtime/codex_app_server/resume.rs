// Preserve exec launch settings when app-server startup fails before a turn starts.
// Explicit resume refusal selects a fresh session with the existing retry prompt.

use crate::{store::Store, types::TaskId};
use anyhow::Result;
use std::process::Command;

pub(crate) fn exec_fallback(cmd: &Command, store: &Store, task: &TaskId) -> Result<Command> {
    let refused = store.get_events(task.as_str())?.iter().any(|event| {
        event.metadata.as_ref().is_some_and(|metadata| {
            metadata["codex_app_server_fallback"] == true
                && metadata["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("Codex thread/resume:"))
        })
    });
    Ok(clone_command(cmd, refused))
}

fn clone_command(cmd: &Command, fresh: bool) -> Command {
    let mut command = Command::new(cmd.get_program());
    let args: Vec<_> = cmd.get_args().collect();
    let mut skip_session = fresh && args.get(1).is_some_and(|arg| *arg == "resume");
    let mut option_value = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_session && index == 1 {
            continue;
        }
        if skip_session && index > 1 && !option_value && !arg.to_string_lossy().starts_with('-') {
            skip_session = false;
            continue;
        }
        const OPTIONS: &[&str] = &[
            "-c",
            "--config",
            "-m",
            "--model",
            "-o",
            "--output-last-message",
            "-C",
            "--cd",
        ];
        option_value = !option_value && arg.to_str().is_some_and(|arg| OPTIONS.contains(&arg));
        command.arg(arg);
    }
    if let Some(dir) = cmd.get_current_dir() {
        command.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    command
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
