// Antigravity CLI (`agy`) adapter: non-streaming, plain-text output.
// agy 1.0.0 has no stream-json / no model flag / no plan mode — kept minimal
// on purpose; revisit when upstream gains those features.

use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::RunOpts;
use crate::types::*;

pub struct AntigravityAgent;

impl super::Agent for AntigravityAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Antigravity
    }

    fn streaming(&self) -> bool {
        false
    }

    fn needs_pty(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("agy");
        cmd.args(["-p", prompt, "--print-timeout", "60m"]);
        // agy 1.0 has no plan/read-only flag; without permission skipping it will prompt.
        if !opts.read_only {
            cmd.arg("--dangerously-skip-permissions");
        }
        // note: opts.model is ignored — agy 1.0 has no model flag.
        for dir in agy_include_directories(opts.dir.as_deref(), &opts.context_files) {
            cmd.args(["--add-dir", &dir]);
        }
        if let Ok(log_file) = std::env::var("AGY_LOG_FILE") {
            if !log_file.is_empty() {
                cmd.args(["--log-file", &log_file]);
            }
        }
        if let Some(ref dir) = opts.dir {
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    }
}

fn agy_include_directories(dir: Option<&str>, context_files: &[String]) -> Vec<String> {
    let mut directories = BTreeSet::new();
    if let Some(run_dir) = dir {
        if !run_dir.is_empty() {
            directories.insert(run_dir.to_string());
        }
    }
    for file in context_files {
        if let Some(include_dir) = context_include_directory(dir, file) {
            directories.insert(include_dir);
        }
    }
    directories.into_iter().collect()
}

fn context_include_directory(run_dir: Option<&str>, context_file: &str) -> Option<String> {
    if context_file.is_empty() {
        return run_dir.map(ToOwned::to_owned);
    }
    let path = Path::new(context_file);
    let include_path = if path.is_dir() {
        path.to_path_buf()
    } else if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        parent.to_path_buf()
    } else {
        PathBuf::from(run_dir.unwrap_or("."))
    };
    Some(include_path.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "antigravity_tests.rs"]
mod tests;
