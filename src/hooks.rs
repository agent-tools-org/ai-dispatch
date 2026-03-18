// Task lifecycle hooks loader and runner for aid tasks.
// Exports load/parse helpers plus runtime execution helpers.
// Deps: anyhow, serde, serde_json, std::process, crate::paths.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::paths;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hook {
    pub event: String,
    pub command: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(skip)]
    trusted: bool,
}

#[derive(Debug, Deserialize)]
struct HooksFile {
    #[serde(rename = "hook", default)]
    hook: Vec<Hook>,
}

pub fn load_hooks() -> Result<Vec<Hook>> {
    let path = paths::aid_dir().join("hooks.toml");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let config: HooksFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse hooks file {}", path.display()))?;
    Ok(config.hook.into_iter().map(|hook| hook.trusted()).collect())
}

pub fn parse_cli_hooks(specs: &[String]) -> Result<Vec<Hook>> {
    specs
        .iter()
        .map(|spec| {
            let (event, command) = spec
                .split_once(':')
                .with_context(|| format!("invalid hook spec '{}': expected event:command", spec))?;
            let event = event.trim();
            let command = command.trim();
            anyhow::ensure!(!event.is_empty(), "hook spec missing event");
            anyhow::ensure!(!command.is_empty(), "hook spec missing command");
            Ok(Hook {
                event: event.to_string(),
                command: command.to_string(),
                agent: None,
                trusted: true,
            })
        })
        .collect()
}

pub fn run_hooks_with(
    event: &str,
    task_json: &Value,
    agent: Option<&str>,
    hooks: &[Hook],
    fail_on_error: bool,
) -> Result<()> {
    let payload = serde_json::to_string(task_json)?;
    let relevant: Vec<&Hook> = hooks
        .iter()
        .filter(|hook| hook.event.eq_ignore_ascii_case(event))
        .filter(|hook| match (&hook.agent, agent) {
            (Some(hook_agent), Some(agent_name)) => hook_agent.eq_ignore_ascii_case(agent_name),
            (Some(_), None) => false,
            _ => true,
        })
        .collect();
    if relevant.is_empty() {
        return Ok(());
    }
    for hook in relevant {
        ensure_trusted_hook(hook)?;
        aid_info!("[aid] Executing hook via shell: {}", hook.command);
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&hook.command);
        cmd.stdin(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to run hook {}", hook.command))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(payload.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if !output.stderr.is_empty() {
            aid_warn!(
                "[aid] Hook {} stderr:\n{}",
                hook.command,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if !output.status.success() {
            let msg = format!("Hook {} exited with {}", hook.command, output.status);
            if fail_on_error {
                anyhow::bail!(msg);
            }
            aid_warn!("[aid] {msg}");
        }
    }
    Ok(())
}

impl Hook {
    pub(crate) fn new_trusted(event: String, command: String, agent: Option<String>) -> Self {
        Self {
            event,
            command,
            agent,
            trusted: true,
        }
    }

    pub(crate) fn trusted(mut self) -> Self {
        self.trusted = true;
        self
    }
}

fn ensure_trusted_hook(hook: &Hook) -> Result<()> {
    anyhow::ensure!(
        hook.trusted,
        "refusing to execute untrusted hook command from task data or agent output"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_cli_hooks_marks_hooks_trusted() {
        let hooks = parse_cli_hooks(&["before_run:echo ok".to_string()]).unwrap();
        assert!(run_hooks_with("before_run", &json!({}), None, &hooks, true).is_ok());
    }

    #[test]
    fn run_hooks_rejects_untrusted_hooks() {
        let err = run_hooks_with(
            "before_run",
            &json!({}),
            None,
            &[Hook {
                event: "before_run".to_string(),
                command: "echo bad".to_string(),
                agent: None,
                trusted: false,
            }],
            true,
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("refusing to execute untrusted hook command"));
    }
}
