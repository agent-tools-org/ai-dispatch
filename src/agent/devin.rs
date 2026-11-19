// Devin CLI adapter: builds `devin --print` commands; stdout is the plain-text answer.
// Runs in-process so the agent dies with its PTY/pipe; liveness comes from tool hooks.
// Deps: serde_json, read_only prompt helpers, home isolation (real user config path).

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::RunOpts;
use crate::types::*;

pub struct DevinAgent;

const CONFIG_FILE: &str = "devin-config.json";
const HOOK_EVENTS: [&str; 2] = ["PreToolUse", "PostToolUse"];

impl super::Agent for DevinAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Devin
    }

    fn streaming(&self) -> bool {
        false
    }

    /// Print mode never reads stdin, so steering and nudges must not be written to it.
    fn accepts_interactive_input(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let allow_result = super::read_only::allow_result_file_write(opts);
        let effective_prompt = if allow_result {
            super::read_only::read_only_prompt(&prompt_with_ctx, opts)
        } else {
            prompt_with_ctx
        };
        // `auto` rejects every tool that needs confirmation in print mode, which is
        // Devin's read-only behaviour. A result file needs a write, so it falls back
        // to the prompt-level read-only contract above.
        let permission = if opts.read_only && !allow_result { "auto" } else { "dangerous" };
        let mut cmd = Command::new("devin");
        // `--print=` keeps a prompt that starts with `-` from parsing as a flag.
        cmd.arg(format!("--print={effective_prompt}"));
        cmd.args(["--permission-mode", permission]);
        // Print mode fails in an untrusted directory, and every aid worktree is new.
        cmd.args(["--respect-workspace-trust", "false"]);
        // The default daemon outlives this process; in-process ties the agent to it.
        cmd.args(["--process-model", "in-process"]);
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        if let Some(config) = super::agent_log_from_opts(opts).and_then(|log| liveness_config(Path::new(log))) {
            cmd.arg("--config").arg(config);
        }
        if let Some(ref dir) = opts.dir {
            if !Path::new(dir).is_dir() {
                bail!("Workspace path does not exist: {dir}");
            }
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let Some(output) = super::model_validation::run_probe_cmd(
            "devin", &["models", "--process-model", "in-process"],
        ) else {
            return Ok(None);
        };
        let models = parse_devin_models_output(&output.stdout);
        Ok(if models.is_empty() { None } else { Some(models) })
    }
}

/// Print mode writes nothing until it exits, and its own log path is not settable.
/// Tool hooks append one line per tool call to the log aid watches, so a long run
/// is not mistaken for a dead spawn. `--config` replaces the user config, so the
/// hooks are merged into a copy of it; an unreadable config gets no hooks rather
/// than silently losing the user's settings.
fn liveness_config(log_file: &Path) -> Option<PathBuf> {
    let user_config = super::home_isolation::resolve_real_home()
        .ok()?
        .join(".config/devin/config.json");
    let base = match std::fs::read_to_string(&user_config) {
        Ok(raw) => serde_json::from_str(&raw).ok()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(_) => return None,
    };
    let config = with_liveness_hooks(base, log_file)?;
    let dir = log_file.parent()?;
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(CONFIG_FILE);
    std::fs::write(&path, serde_json::to_vec_pretty(&config).ok()?).ok()?;
    Some(path)
}

pub(crate) fn with_liveness_hooks(mut config: Value, log_file: &Path) -> Option<Value> {
    let log = log_file.to_str().filter(|path| !path.contains('\''))?;
    let hooks = config
        .as_object_mut()?
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()?;
    for event in HOOK_EVENTS {
        // Only the event name is logged: tool output in the diagnostic log would be
        // scanned for refusals and could hold a provider on a quoted error.
        let command = format!("cat >/dev/null; echo \"devin {event}\" >> '{log}'");
        let entry = json!({ "hooks": [{ "type": "command", "command": command, "timeout": 5 }] });
        hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()?
            .push(entry);
    }
    Some(config)
}

/// `devin models` lists `Family (family-id)` headers, optional `aliases:` lines,
/// and indented model ids. `--model` accepts all three forms.
pub(crate) fn parse_devin_models_output(output: &str) -> Vec<String> {
    let mut models: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !models.iter().any(|known| known == name) {
            models.push(name.to_string());
        }
    };
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Available models") {
            continue;
        }
        if !line.starts_with(' ') {
            if let Some(family) = trimmed
                .rsplit_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
            {
                push(family.trim());
            }
            continue;
        }
        if let Some(aliases) = trimmed.strip_prefix("aliases:") {
            aliases.split(',').for_each(|alias| push(alias.trim()));
            continue;
        }
        push(trimmed.split_whitespace().next().unwrap_or_default());
    }
    models
}

#[cfg(test)]
#[path = "devin_tests.rs"]
mod tests;
