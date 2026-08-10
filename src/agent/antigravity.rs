// Antigravity CLI (`agy`) adapter: non-streaming, plain-text output.
// Probes runtime CLI capabilities once, then builds the safest command shape
// supported by the installed agy version.

use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::read_only::read_only_prompt;
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

    fn accepts_interactive_input(&self) -> bool {
        false
    }

    fn needs_pty(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let caps = agy_capabilities();
        let plan_flag = caps.plan_mode_flag;
        let allow_result = super::read_only::allow_result_file_write(opts);
        // Plan mode blocks result-file delivery; fall back to prompt-level RO.
        let use_plan = opts.read_only && plan_flag.is_some() && !allow_result;
        let effective_prompt = if opts.read_only && !use_plan {
            if plan_flag.is_none() {
                aid_warn!("[aid] agy read-only is prompt-level only, not enforced. Use --worktree or --sandbox for isolation.");
            }
            read_only_prompt(prompt, opts)
        } else {
            prompt.to_string()
        };
        let mut cmd = Command::new("agy");
        if use_plan && let Some(flag) = plan_flag {
            cmd.args([flag, "plan"]);
        }
        if let Some(ref model) = opts.model {
            if caps.has_model_flag {
                cmd.args(["--model", model]);
            } else {
                aid_warn!(
                    "[aid] agy {} has no model flag; ignoring --model {model}",
                    agy_version_string().unwrap_or_else(|| "1.0".into())
                );
            }
        }
        cmd.arg("-p");
        cmd.arg(&effective_prompt);
        cmd.args(["--print-timeout", "24h"]);
        cmd.arg("--dangerously-skip-permissions");
        let run_dir = opts
            .dir
            .as_deref()
            .filter(|dir| !dir.is_empty())
            .and_then(|dir| absolute_dir(Path::new(dir)));
        for dir in agy_include_directories(run_dir.as_deref(), &opts.context_files) {
            cmd.args(["--add-dir", &dir]);
        }
        // agy runs in print mode: it emits nothing on stdout until a turn completes, so
        // aid's "no output since spawn" liveness check cannot tell a long first turn from
        // a dead process and reaps healthy runs at the first-token budget. agy's own log
        // does grow throughout, so point it at a per-task path aid can watch.
        // AGY_LOG_FILE stays an operator override.
        // agy runs in print mode: nothing reaches stdout until a turn completes, so aid's
        // "no output since spawn" check cannot tell a long first turn from a dead process
        // and reaps healthy runs at the first-token budget. agy's own log grows throughout.
        // The caller decides whether that path is watchable; this adapter just uses it.
        if let Some(log_file) = super::agent_log_from_opts(opts) {
            if let Some(parent) = std::path::Path::new(log_file).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            cmd.args(["--log-file", log_file]);
        }

        if let Some(ref dir) = run_dir {
            // The sandbox and container wrappers mount this cwd verbatim (`-v dir:dir`),
            // so it must name the same directory as the workspace paths above -
            // otherwise the mount and `--add-dir` disagree and agy loses the workspace.
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let mut cmd = Command::new("agy");
        cmd.arg("models");
        let Some(output) = super::model_validation::run_cmd_with_timeout(cmd, std::time::Duration::from_secs(2)) else {
            return Ok(None);
        };
        let models = parse_agy_models_output(&output);
        Ok(if models.is_empty() { None } else { Some(models) })
    }
}

fn parse_agy_models_output(output: &str) -> Vec<String> {
    let mut models = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("Available") {
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        let clean = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '.');
        if !clean.is_empty() && !models.contains(&clean.to_string()) {
            models.push(clean.to_string());
        }
    }
    models
}

#[derive(Debug, Clone, Default)]
struct AgyCapabilities {
    /// Flag name that puts agy in plan (read-only) mode, if the CLI supports one.
    /// agy >= 1.1 spells it `--mode`; older builds used `--approval-mode`.
    plan_mode_flag: Option<&'static str>,
    has_model_flag: bool,
}

fn agy_capabilities() -> &'static AgyCapabilities {
    static CAPS: OnceLock<AgyCapabilities> = OnceLock::new();
    CAPS.get_or_init(|| match probe_agy_capabilities() {
        Some(caps) => caps,
        None => {
            aid_warn!("[aid] failed to probe agy capabilities; assuming no optional agy flags");
            AgyCapabilities::default()
        }
    })
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
    Some(parse_agy_capabilities(&help))
}

fn parse_agy_capabilities(help: &str) -> AgyCapabilities {
    let plan_mode_flag = if help_defines_flag(help, "--approval-mode") {
        Some("--approval-mode")
    } else if help_defines_flag(help, "--mode") {
        Some("--mode")
    } else {
        None
    };
    AgyCapabilities {
        plan_mode_flag,
        has_model_flag: help_defines_flag(help, "--model"),
    }
}

/// Does the help text *define* this flag, rather than merely mention it? A bare
/// `contains` matches prefixes (`--model` inside `--model-fallback`) and prose in another
/// flag's description, either of which can pick a flag the installed agy does not accept.
fn help_defines_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix(flag).is_some_and(|rest| {
            rest.is_empty() || rest.starts_with([' ', '\t', '=', ','])
        })
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

/// agy rejects any non-absolute `--add-dir` ("must be an absolute path") and keeps the
/// unresolved entry in its workspace list, which leaves its Search tool without an app
/// root for the whole session. Every path handed to agy must therefore be absolutized.
fn agy_include_directories(run_dir: Option<&Path>, context_files: &[String]) -> Vec<String> {
    let mut directories = BTreeSet::new();
    if let Some(run_dir) = run_dir {
        directories.insert(run_dir.to_string_lossy().into_owned());
    }
    for file in context_files {
        if let Some(include_dir) = context_include_directory(run_dir, file) {
            directories.insert(include_dir);
        }
    }
    directories.into_iter().collect()
}

fn context_include_directory(run_dir: Option<&Path>, context_file: &str) -> Option<String> {
    if context_file.is_empty() {
        return run_dir.map(|run_dir| run_dir.to_string_lossy().into_owned());
    }
    let path = Path::new(context_file);
    let include_path = if path.is_dir() {
        path.to_path_buf()
    } else if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        parent.to_path_buf()
    } else {
        // Bare filename: it lives in the agent's cwd.
        PathBuf::from(".")
    };
    // Context paths are relative to the run directory, which is the agent's cwd.
    let include_path = match (include_path.is_absolute(), run_dir) {
        (false, Some(run_dir)) => run_dir.join(include_path),
        _ => include_path,
    };
    absolute_dir(&include_path).map(|path| path.to_string_lossy().into_owned())
}

/// Absolutize, resolving symlinks when the directory exists so that `/tmp/x` and
/// `/private/tmp/x` collapse to one workspace entry. Falls back to a lexical
/// absolutization for paths that do not exist yet. Returns `None` only when the process
/// has no usable cwd, in which case the entry is dropped rather than handed to agy as a
/// path it will reject.
fn absolute_dir(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path)
        .ok()
        .or_else(|| std::path::absolute(path).ok())
}

#[cfg(test)]
#[path = "antigravity_tests.rs"]
mod tests;
