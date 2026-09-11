// Cursor Agent CLI adapter: builds `agent`/`cursor-agent` commands, parses stream-json output.
// Uses the standalone Cursor binary, preferring `agent` over the legacy alias.

use anyhow::Result;
use chrono::Local;
#[cfg(test)]
use std::cell::RefCell;
use std::process::Command;
#[cfg(not(test))]
use std::sync::OnceLock;

use super::truncate::capped_detail;
#[path = "cursor_events.rs"]
mod events;
use events::parse_json_event;
use super::RunOpts;
use crate::types::*;

pub struct CursorAgent;

/// Cursor renamed `cursor-agent` to `agent`, so `agent` stays the preferred name — but it
/// is far too generic to take on faith. xAI's Grok Build CLI installs a binary called
/// exactly that, and handing Cursor's flags to it fails instantly with an unrelated
/// argument error that reads like a Cursor bug. Accept `agent` only when it says it is
/// Cursor's, and fall back to the unambiguous alias otherwise.
fn cursor_binary() -> &'static str {
    #[cfg(test)]
    if let Some(binary) = TEST_CURSOR_BINARY.with(|cell| *cell.borrow()) {
        return binary;
    }

    #[cfg(not(test))]
    {
        static RESOLVED: OnceLock<&'static str> = OnceLock::new();
        return *RESOLVED.get_or_init(resolve_cursor_binary);
    }

    #[cfg(test)]
    resolve_cursor_binary()
}

fn resolve_cursor_binary() -> &'static str {
    resolve_cursor_binary_from_path(std::env::var_os("PATH"), identifies_as_cursor)
}

/// Walk PATH for an executable `agent` that passes `is_cursor`; else `cursor-agent`.
/// Returns an absolute path when a Cursor `agent` wins so dispatch bypasses PATH order.
fn resolve_cursor_binary_from_path(
    path: Option<std::ffi::OsString>,
    is_cursor: impl FnMut(&str) -> bool,
) -> &'static str {
    match super::env_identity::first_matching_executable(path.as_deref(), "agent", is_cursor) {
        Some(found) => Box::leak(found.into_boxed_str()),
        None => "cursor-agent",
    }
}

fn identifies_as_cursor(binary: &str) -> bool {
    super::env_identity::binary_identity_matches(binary, "cursor")
}

fn help_mentions_cursor(help: &str) -> bool {
    help.to_ascii_lowercase().contains("cursor")
}

#[cfg(test)]
thread_local! {
    static TEST_CURSOR_BINARY: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct CursorBinaryGuard {
    previous: Option<&'static str>,
}

#[cfg(test)]
impl CursorBinaryGuard {
    pub(crate) fn set(binary: &'static str) -> Self {
        let previous = TEST_CURSOR_BINARY.with(|cell| cell.replace(Some(binary)));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for CursorBinaryGuard {
    fn drop(&mut self) {
        TEST_CURSOR_BINARY.with(|cell| cell.replace(self.previous.take()));
    }
}

impl super::Agent for CursorAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Cursor
    }

    fn default_model(&self) -> Option<String> {
        Some("composer-2.5".to_string())
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new(cursor_binary());
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let effective_prompt = if super::read_only::allow_result_file_write(opts) {
            super::read_only::read_only_prompt(&prompt_with_ctx, opts)
        } else {
            prompt_with_ctx
        };
        // Cursor documents stream-json "assistant" events as deltas; only the terminal "result"
        // event is complete, so requesting --stream-partial-output just degrades logs into tokens.
        // Plan mode cannot write the audit result file; keep --force and prompt-level read-only.
        if opts.read_only && !super::read_only::allow_result_file_write(opts) {
            cmd.args([
                "-p",
                "--trust",
                &effective_prompt,
                "--mode",
                "plan",
                "--output-format",
                "stream-json",
            ]);
        } else {
            cmd.args([
                "-p",
                &effective_prompt,
                "--trust",
                "--force",
                "--output-format",
                "stream-json",
            ]);
        }
        if let Some(ref dir) = opts.dir {
            let path = std::path::Path::new(dir);
            if !path.is_dir() {
                anyhow::bail!("Workspace path does not exist: {dir}");
            }
            cmd.args(["--workspace", dir]);
            cmd.current_dir(dir);
        }
        if let Some(model) = opts.model.clone().or_else(|| self.default_model()) {
            cmd.args(["--model", &model]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let now = Local::now();

        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return parse_json_event(task_id, &v, now);
        }

        let (kind, detail) = classify_line(trimmed);
        kind.map(|k| {
            let (detail, metadata) = capped_detail(detail);
            TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: k,
                detail,
                metadata,
            }
        })
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        // Real Cursor success ends with type:result + is_error:false; failures set is_error:true.
        super::stream_completion::status_from_result_jsonl(output)
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let binary = cursor_binary();
        let mut cmd = Command::new(binary);
        cmd.arg("models");
        let output = super::model_validation::run_probe_cmd(cmd);
        let Some(probe) = output else {
            return Ok(None);
        };
        let mut models = parse_cursor_models_output(&probe.stdout);
        for alias in crate::types::ROUTER_ALIASES {
            if !models.iter().any(|m| m.eq_ignore_ascii_case(alias)) {
                models.push((*alias).to_string());
            }
        }
        Ok(Some(models))
    }
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                i = j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn parse_cursor_models_output(output: &str) -> Vec<String> {
    let mut models = Vec::new();
    let cleaned = strip_ansi(output);
    for line in cleaned.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('<') {
            continue;
        }
        let name = trimmed.split_whitespace().next().unwrap_or("");
        if !name.is_empty() && !models.contains(&name.to_string()) {
            models.push(name.to_string());
        }
    }
    models
}

/// Classification only. This once also wrote rate-limit markers, off a `detail`
/// that is the model's own assistant text on one branch and an aid-composed tool
/// line (`completed: grep <pattern>`) on another — neither is the provider
/// speaking, and both wrote real holds on a cursor that was serving. Cursor's
/// two captured refusals are read where they actually arrive: the workspace
/// quota as a `{"type":"error"}` line on the stream, and the spent premium pool
/// as `ActionRequiredError` on stderr. Both go through `quota_channel`.
fn classify_line(line: &str) -> (Option<EventKind>, &str) {
    if is_error_line(line) {
        (Some(EventKind::Error), line)
    } else if line.contains("test result:") || (line.contains("running") && line.contains("test")) {
        (Some(EventKind::Test), line)
    } else if line.contains("Compiling") || line.contains("Finished") {
        (Some(EventKind::Build), line)
    } else if line.contains("git commit") {
        (Some(EventKind::Commit), line)
    } else if line.starts_with("Writing") || line.starts_with("Creating") || line.contains("wrote")
    {
        (Some(EventKind::FileWrite), line)
    } else if line.starts_with("Reading") {
        (Some(EventKind::FileRead), line)
    } else if line.len() > 10 {
        (Some(EventKind::Reasoning), line)
    } else {
        (None, line)
    }
}

fn is_error_line(line: &str) -> bool {
    line.contains("error[") || line.contains("FAILED") || line.starts_with("Error:")
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
