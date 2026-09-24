// Codex CLI adapter: builds `codex exec` commands and parses JSONL event streams.
// Exports CodexAgent for streaming runs plus helpers for tool and usage events.
// Depends on serde_json for metadata-rich completion events.

mod capabilities;
pub(crate) mod cli_config;
mod output_classifier;
mod events;
mod session;
use events::{parse_item_event, parse_turn_completed, parse_thread_started, parse_error_event};
pub(crate) use session::{resume_fallback_needed, resume_fallback_event, rollout_filename_matches_for_attribution};
#[cfg(test)]
use session::{RESUME_FALLBACK_DETAIL, rollout_filename_matches, session_rollout_exists};
#[path = "codex_attribution.rs"]
mod attribution;

pub(crate) use attribution::grade_completion_observation;

use anyhow::{bail, Result};
use chrono::{Local, NaiveDateTime};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use output_classifier::classify_output;
use super::read_only::read_only_prompt;
use super::truncate::{capped_detail, capped_detail_with, truncate_text};
use super::{CommandContext, RunOpts};
use crate::templates;
use crate::types::*;

/// Parsed codex CLI version (major, minor, patch), when the probe succeeds.
/// Cached via OnceLock so `codex --version` runs at most once.
fn codex_version() -> Option<(u32, u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        Command::new("codex")
            .arg("--version")
            .output()
            .ok()
            .and_then(|out| {
                if !out.status.success() {
                    return None;
                }
                let text = String::from_utf8_lossy(&out.stdout);
                parse_semver(text.trim())
            })
    })
}

fn parse_semver(text: &str) -> Option<(u32, u32, u32)> {
    // "codex-cli 0.116.0" → "0.116.0"
    let ver = text.rsplit(' ').next()?;
    let mut parts = ver.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true if codex CLI supports the native `-m` / `--model` flag (≥ 0.116.0).
fn has_native_model_flag() -> bool {
    codex_version().is_some_and(|version| version >= (0, 116, 0))
}

pub struct CodexAgent;

impl CodexAgent {
    fn build_codex_command(
        &self,
        prompt: &str,
        opts: &RunOpts,
        durable_codex_home: bool,
        writable_roots: &[std::path::PathBuf],
    ) -> Result<Command> {
        let effective_prompt = if opts.read_only {
            read_only_prompt(prompt, opts)
        } else {
            prompt.to_string()
        };
        let with_context = super::embed_context_in_prompt(&effective_prompt, &opts.context_files)?;
        let injected = templates::inject_codex_prompt(&with_context, None);
        let mut cmd = Command::new("codex");
        let resume_session_id = opts
            .session_id
            .as_deref()
            .filter(|session_id| !durable_codex_home || !resume_fallback_needed(session_id));
        if let Some(session_id) = resume_session_id {
            cmd.args([
                "exec",
                "resume",
                "--json",
                "--skip-git-repo-check",
                session_id,
                &injected,
            ]);
        } else {
            cmd.args(["exec", "--json", "--skip-git-repo-check"]);
            if let Some(version) = codex_version() {
                cmd.arg(capabilities::approval_flag_for_version(version).as_str());
            }
            cmd.arg(&injected);
        }
        if let Some(ref model) = opts.model {
            if has_native_model_flag() {
                cmd.args(["-m", model]);
            } else {
                cmd.args(["-c", &format!("model=\"{model}\"")]);
            }
        }
        if let Some(ref output) = opts.output {
            cmd.args(["-o", output]);
        }
        if let Some(ref dir) = opts.dir {
            let dir_path = Path::new(dir);
            if !dir_path.exists() {
                bail!("codex working directory does not exist: {}", dir);
            }
            if resume_session_id.is_none() {
                cmd.args(["-C", dir]);
            }
            cmd.current_dir(dir);
        }
        super::scratch::grant_codex_roots(&mut cmd, writable_roots)?;
        Ok(cmd)
    }
}

impl super::Agent for CodexAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
        true
    }

    fn accepts_idle_nudge(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let roots = super::scratch::writable_roots(opts.dir.as_deref());
        self.build_codex_command(prompt, opts, true, &roots)
    }

    fn validate_cli(&self) -> Result<()> {
        capabilities::validate_installed_codex(codex_version())
    }

    fn validate_cli_with(&self, run: &crate::agent::CliCommandRunner<'_>) -> Result<()> {
        capabilities::validate_installed_codex_with(codex_version(), run)
    }

    fn build_command_with_context(
        &self,
        prompt: &str,
        opts: &RunOpts,
        context: CommandContext,
    ) -> Result<Command> {
        self.build_codex_command(
            prompt,
            opts,
            context.durable_codex_home,
            &context.writable_roots,
        )
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let now = Local::now();

        // Check for NO_CHANGES_NEEDED in any text content
        if line.contains("NO_CHANGES_NEEDED") {
            return Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::NoOp,
                detail: extract_noop_reason(line),
                metadata: None,
            });
        }

        let event_type = v.get("type")?.as_str()?;
        match event_type {
            "item.started" | "item.completed" => parse_item_event(task_id, &v, now),
            "turn.completed" => parse_turn_completed(task_id, &v, now),
            "thread.started" => parse_thread_started(task_id, &v, now),
            "error" => parse_error_event(task_id, &v, now),
            _ => None,
        }
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let cache_path = std::path::Path::new(&home).join(".codex/models_cache.json");
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
                    let slugs: Vec<String> = arr
                        .iter()
                        .filter_map(|item| item.get("slug").and_then(|s| s.as_str()).map(String::from))
                        .collect();
                    if !slugs.is_empty() {
                        return Ok(Some(slugs));
                    }
                }
            }
        }
        Ok(None)
    }
}

fn classify_command(command: &str) -> EventKind {
    if command.contains("cargo test") || command.contains("npm test") {
        EventKind::Test
    } else if command.contains("cargo build") || command.contains("cargo check") {
        EventKind::Build
    } else if command.contains("git commit") {
        EventKind::Commit
    } else if command.contains("cargo fmt") || command.contains("prettier") {
        EventKind::Format
    } else if command.contains("cargo clippy") || command.contains("eslint") {
        EventKind::Lint
    } else {
        EventKind::ToolCall
    }
}

fn extract_noop_reason(line: &str) -> String {
    if let Some(pos) = line.find("NO_CHANGES_NEEDED:") {
        let reason = &line[pos + 18..];
        format!("NO_CHANGES_NEEDED:{}", reason.trim().trim_matches('"'))
    } else {
        "NO_CHANGES_NEEDED".to_string()
    }
}

#[cfg(test)]
#[path = "codex_writable_roots_tests.rs"]
mod writable_roots_tests;

#[cfg(test)]
#[path = "codex_quota_tests.rs"]
mod quota_tests;

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "codex_roots_tests.rs"]
mod roots_tests;
#[cfg(test)]
#[path = "codex_command_tests.rs"]
mod command_tests;

#[cfg(test)]
mod codex_nudge_tests;
