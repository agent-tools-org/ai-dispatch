// Antigravity CLI (`agy`) adapter: non-streaming, plain-text output.
// Probes runtime CLI capabilities once, then builds the safest command shape
// supported by the installed agy version.

use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

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
        if opts.read_only {
            let caps = agy_capabilities();
            if !caps.has_plan_mode {
                if opts.sandbox && crate::sandbox::can_sandbox(AgentKind::Antigravity) {
                    aid_warn!(
                        "[aid] agy has no plan mode; relying on --sandbox container for read-only enforcement"
                    );
                } else if crate::sandbox::is_available()
                    && crate::sandbox::can_sandbox(AgentKind::Antigravity)
                {
                    anyhow::bail!(
                        "agy 1.0 has no plan mode. Rerun with --sandbox (container available) \
                         to let aid enforce read-only, or use `gemini` (--approval-mode plan)."
                    );
                } else {
                    anyhow::bail!(
                        "agy 1.0 has no plan mode and no container sandbox is available. \
                         Use `gemini` for read-only audit tasks (--approval-mode plan), \
                         or install Apple's container CLI and rerun with --sandbox."
                    );
                }
            }
        }
        let mut cmd = Command::new("agy");
        if opts.read_only && agy_capabilities().has_plan_mode {
            cmd.args(["--approval-mode", "plan"]);
        }
        if let Some(ref model) = opts.model {
            let caps = agy_capabilities();
            if caps.has_model_flag {
                cmd.args(["-m", model]);
            } else {
                aid_warn!(
                    "[aid] agy {} has no model flag; ignoring --model {model}",
                    agy_version_string().unwrap_or_else(|| "1.0".into())
                );
            }
        }
        cmd.args(["-p", prompt, "--print-timeout", "24h"]);
        cmd.arg("--dangerously-skip-permissions");
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
            model: Some("gemini-3-pro-preview".to_string()),
            cost_usd: Some(0.0),
            exit_code: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct AgyCapabilities {
    has_plan_mode: bool,
    has_model_flag: bool,
    has_stream_json: bool,
}

fn agy_capabilities() -> &'static AgyCapabilities {
    static CAPS: OnceLock<AgyCapabilities> = OnceLock::new();
    CAPS.get_or_init(|| probe_agy_capabilities().unwrap_or_default())
}

#[cfg(not(test))]
fn probe_agy_capabilities() -> Option<AgyCapabilities> {
    let output = std::process::Command::new("agy")
        .arg("--help")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let help = format!("{stdout}{stderr}");
    Some(AgyCapabilities {
        has_plan_mode: help.contains("--approval-mode") || help.contains("--plan"),
        has_model_flag: help.contains("-m ") || help.contains("--model"),
        has_stream_json: help.contains("stream-json"),
    })
}

#[cfg(test)]
fn probe_agy_capabilities() -> Option<AgyCapabilities> {
    None
}

fn agy_version_string() -> Option<String> {
    let output = std::process::Command::new("agy")
        .arg("--version")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
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
