// Translate the existing Codex exec launch into app-server thread parameters.
// Preserves launch environment and native permission configuration without shell parsing.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::process::Command;

pub(super) struct Launch {
    pub command: Command,
    pub thread: Value,
    pub prompt: String,
    pub resume: bool,
}

pub(crate) fn available(cmd: &Command) -> bool {
    let Some(program) = cmd.get_program().to_str() else {
        return false;
    };
    if std::path::Path::new(program)
        .file_name()
        .is_none_or(|name| name != "codex")
    {
        return false;
    }
    crate::agent::env_identity::run_bounded(
        program,
        &["app-server", "--help"],
        std::time::Duration::from_secs(2),
    )
    .is_ok_and(|output| output.success && output.stdout.contains("stdio"))
}

pub(super) fn translate(cmd: &Command) -> Result<Launch> {
    let args: Vec<_> = cmd
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    anyhow::ensure!(
        args.first().is_some_and(|arg| arg == "exec"),
        "Expected Codex exec launch"
    );
    let resume = args.get(1).is_some_and(|arg| arg == "resume");
    let mut launch = Launch {
        command: Command::new(cmd.get_program()),
        thread: json!({"approvalPolicy":"never"}),
        prompt: String::new(),
        resume,
    };
    launch.command.arg("app-server");
    if let Some(dir) = cmd.get_current_dir() {
        launch.command.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            launch.command.env(key, value);
        } else {
            launch.command.env_remove(key);
        }
    }
    parse_args(&mut launch, &args[if resume { 2 } else { 1 }..])?;
    Ok(launch)
}

fn parse_args(launch: &mut Launch, args: &[String]) -> Result<()> {
    let mut positional = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" | "--skip-git-repo-check" => {}
            "--ignore-user-config" => {
                bail!("Codex app-server cannot preserve --ignore-user-config isolation")
            }
            "-c" | "--config" => {
                let value = args.get(index + 1).context("Missing Codex config value")?;
                // App-server accepts the same global TOML overrides as exec.
                launch.command.args(["-c", value]);
                index += 1;
            }
            "-m" | "--model" | "-C" | "--cd" | "-o" | "--output-last-message" => {
                let value = args.get(index + 1).context("Missing Codex option value")?;
                match args[index].as_str() {
                    "-m" | "--model" => launch.thread["model"] = json!(value),
                    "-C" | "--cd" => launch.thread["cwd"] = json!(value),
                    _ => {}
                }
                index += 1;
            }
            value if value.starts_with('-') => {
                bail!("Unsupported Codex app-server option: {value}")
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    launch.prompt = positional.pop().context("Missing Codex prompt")?;
    if launch.resume {
        launch.thread["threadId"] = json!(positional.pop().context("Missing session")?);
    }
    anyhow::ensure!(
        positional.is_empty(),
        "Unexpected Codex positional arguments"
    );
    if launch.thread.get("cwd").is_none() {
        launch.thread["cwd"] = json!(
            launch
                .command
                .get_current_dir()
                .map(std::path::Path::to_path_buf)
                .unwrap_or(std::env::current_dir()?)
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
