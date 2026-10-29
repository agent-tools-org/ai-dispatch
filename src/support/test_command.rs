// Shared command inspection for adapter and runtime tests.
// Exports: args and env; preserves lossy UTF-8 conversion and explicit env removals.
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
