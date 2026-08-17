// Codex CLI adapter: builds `codex exec` commands and parses JSONL event streams.
// Exports CodexAgent for streaming runs plus helpers for tool and usage events.
// Depends on serde_json for metadata-rich completion events.

mod capabilities;
mod output_classifier;
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
use crate::worktree_layout::{read_commondir, resolve_worktree_gitdir};

pub(crate) const RESUME_FALLBACK_DETAIL: &str =
    "Codex session resume skipped: rollout missing; starting fresh session";
const ROLLOUT_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H-%M-%S";

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

pub(crate) fn durable_session_rollout_exists(session_id: &str) -> bool {
    let Ok(real_home) = super::home_isolation::resolve_real_home() else {
        return false;
    };
    session_rollout_exists(&real_home.join(".codex").join("sessions"), session_id)
}

pub(crate) fn resume_fallback_needed(session_id: &str) -> bool {
    !durable_session_rollout_exists(session_id)
}

pub(crate) fn session_rollout_exists(sessions_dir: &Path, session_id: &str) -> bool {
    let Ok(entries) = fs::read_dir(sessions_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return session_rollout_exists(&path, session_id);
        }
        rollout_filename_matches(&path, session_id)
    })
}

pub(crate) fn rollout_filename_matches(path: &Path, session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = name
        .strip_prefix("rollout-")
        .and_then(|name| name.strip_suffix(".jsonl"))
    else {
        return false;
    };
    let Some(timestamp) = stem.strip_suffix(&format!("-{session_id}")) else {
        return false;
    };
    // Resume safety intentionally depends on Codex's current timestamp-shaped rollout prefix.
    NaiveDateTime::parse_from_str(timestamp, ROLLOUT_TIMESTAMP_FORMAT).is_ok()
}

pub(crate) fn rollout_filename_matches_for_attribution(
    path: &Path,
    session_id: &str,
) -> bool {
    if session_id.is_empty() {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("rollout-") && name.ends_with(&format!("-{session_id}.jsonl"))
        })
}

pub(crate) fn resume_fallback_event(task_id: &TaskId) -> TaskEvent {
    TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail: RESUME_FALLBACK_DETAIL.to_string(),
        metadata: None,
    }
}

pub struct CodexAgent;

impl CodexAgent {
    fn build_codex_command(
        &self,
        prompt: &str,
        opts: &RunOpts,
        durable_codex_home: bool,
        cargo_target_dir: Option<&Path>,
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
            if let Some(gitdir) = resolve_worktree_gitdir(dir_path) {
                if let Some(config) = writable_roots_config(
                    &gitdir,
                    cargo_target_dir,
                ) {
                    cmd.args(["-c", &config]);
                } else {
                    eprintln!(
                        "warning: codex worktree gitdir is not valid UTF-8: {}",
                        gitdir.display()
                    );
                }
            }
            if resume_session_id.is_none() {
                cmd.args(["-C", dir]);
            }
            cmd.current_dir(dir);
        }
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
        self.build_codex_command(prompt, opts, true, None)
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
            context.cargo_target_dir.as_deref().map(Path::new),
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

fn writable_roots_config(path: &Path, cargo_target_dir: Option<&Path>) -> Option<String> {
    let mut roots = vec![toml::Value::String(path.to_str()?.to_string())];
    if let Some(commondir) =
        read_commondir(path).and_then(|path| path.to_str().map(ToOwned::to_owned))
    {
        roots.push(toml::Value::String(commondir));
    }
    if let Some(target_dir) = cargo_target_dir.and_then(|path| path.to_str()) {
        roots.push(toml::Value::String(target_dir.to_string()));
    }
    let value = toml::Value::Array(roots);
    Some(format!("sandbox_workspace_write.writable_roots={value}"))
}

fn parse_item_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let item = v.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "agent_message" => {
            let text = item
                .get("text")
                .or_else(|| item.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return None;
            }
            let (detail, metadata) = capped_detail(text);
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Reasoning,
                detail,
                metadata,
            })
        }
        "command_execution" => parse_command_event(task_id, item, event_type, now),
        "file_change" => parse_file_change_event(task_id, item, now),
        "error" => {
            let message = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if message.is_empty() {
                return None;
            }
            crate::quota_channel::mark_stream_refusal(AgentKind::Codex, None, &item.to_string());
            let (detail, metadata) = capped_detail(message);
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Error,
                detail,
                metadata,
            })
        }
        _ => None,
    }
}

fn parse_command_event(
    task_id: &TaskId,
    item: &Value,
    event_type: &str,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return None;
    }

    if event_type == "item.started" {
        let (detail, metadata) =
            capped_detail_with(command, Some(json!({ "command": command, "status": "in_progress" })));
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: classify_command(command),
            detail,
            metadata,
        });
    }

    let exit_code = item.get("exit_code").and_then(|v| v.as_i64());
    if matches!(exit_code, Some(code) if code != 0) {
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Error,
            detail: format!(
                "command failed ({}) {}",
                exit_code.unwrap_or(-1),
                truncate_text(command, 60)
            ),
            metadata: Some(json!({ "command": command, "exit_code": exit_code })),
        });
    }

    let output = item
        .get("aggregated_output")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let event_kind = classify_output(output)?;
    let (detail, metadata) =
        capped_detail_with(output, Some(json!({ "command": command, "exit_code": exit_code })));
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail,
        metadata,
    })
}

fn parse_turn_completed(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let usage = v.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cached_input_tokens = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let total_tokens = input_tokens + output_tokens;
    let detail = if cached_input_tokens > 0 {
        format!(
            "tokens: {} in + {} out = {} ({} cached)",
            input_tokens, output_tokens, total_tokens, cached_input_tokens
        )
    } else {
        format!(
            "tokens: {} in + {} out = {}",
            input_tokens, output_tokens, total_tokens
        )
    };

    let cost_usd = v.get("cost_usd").and_then(|c| c.as_f64());
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail,
        metadata: Some(completion_metadata(
            total_tokens,
            input_tokens,
            output_tokens,
            cached_input_tokens,
            extract_model(v),
            cost_usd,
        )),
    })
}

fn parse_error_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let detail = v
        .get("message")
        .or_else(|| v.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())?;

    crate::quota_channel::mark_stream_refusal(AgentKind::Codex, None, &v.to_string());

    let (detail, metadata) = capped_detail(detail);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail,
        metadata,
    })
}

fn parse_thread_started(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let thread_id = v.get("thread_id")?.as_str()?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail: format!("session {}", thread_id),
        metadata: Some(json!({ "agent_session_id": thread_id })),
    })
}

fn parse_file_change_event(
    task_id: &TaskId,
    item: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let changes = item.get("changes")?.as_array()?;
    let paths: Vec<&str> = changes
        .iter()
        .filter_map(|c| c.get("path").and_then(|p| p.as_str()))
        .collect();
    if paths.is_empty() {
        return None;
    }
    let text = if paths.len() == 1 {
        paths[0].to_string()
    } else {
        format!("{} files changed", paths.len())
    };
    let (detail, metadata) = capped_detail_with(&text, Some(json!({ "files": paths })));
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::FileWrite,
        detail,
        metadata,
    })
}

fn completion_metadata(
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    model: Option<String>,
    cost_usd: Option<f64>,
) -> Value {
    let mut map = Map::from_iter([
        ("tokens".to_string(), json!(total_tokens)),
        ("input_tokens".to_string(), json!(input_tokens)),
        ("output_tokens".to_string(), json!(output_tokens)),
        (
            "cached_input_tokens".to_string(),
            json!(cached_input_tokens),
        ),
    ]);
    if let Some(value) = model {
        map.insert("model".to_string(), json!(value));
    }
    if let Some(cost) = cost_usd {
        map.insert("cost_usd".to_string(), json!(cost));
    }
    Value::Object(map)
}

fn extract_model(v: &Value) -> Option<String> {
    [
        "/model",
        "/assistant/model",
        "/session/model",
        "/turn/model",
        "/usage/model",
        "/item/model",
    ]
    .iter()
    .find_map(|pointer| v.pointer(pointer).and_then(|value| value.as_str()))
    .map(ToOwned::to_owned)
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
mod tests {
    use super::{
        parse_semver, resume_fallback_event,
        rollout_filename_matches, session_rollout_exists, CodexAgent,
        RESUME_FALLBACK_DETAIL,
    };
    use crate::agent::{Agent, CommandContext, RunOpts};
    use crate::types::{EventKind, TaskId};
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn semver_parsing() {
        assert_eq!(parse_semver("codex-cli 0.116.0"), Some((0, 116, 0)));
        assert_eq!(parse_semver("codex-cli 0.99.3"), Some((0, 99, 3)));
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("garbage"), None);
    }

    #[test]
    fn version_comparison_for_model_flag() {
        assert!((0, 116, 0) >= (0, 116, 0));
        assert!((0, 117, 0) >= (0, 116, 0));
        assert!((1, 0, 0) >= (0, 116, 0));
        assert!((0, 115, 9) < (0, 116, 0));
        assert!((0, 0, 0) < (0, 116, 0));
    }

    #[test]
    fn parses_agent_message_items() {
        let agent = CodexAgent;
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Planning the next edit."}}"#;
        let event = agent
            .parse_event(&TaskId("t-msg".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert!(event.detail.contains("Planning"));
    }

    #[test]
    fn parses_thread_started_session_id() {
        let agent = CodexAgent;
        let line = r#"{"type":"thread.started","thread_id":"019d1efa-5aa6-7132-bdfa-71fb97e12438"}"#;
        let event = agent
            .parse_event(&TaskId("t-thread".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Milestone);
        assert_eq!(
            event
                .metadata
                .unwrap()
                .get("agent_session_id")
                .and_then(|v| v.as_str()),
            Some("019d1efa-5aa6-7132-bdfa-71fb97e12438")
        );
    }

    #[test]
    fn builds_resume_fallback_milestone() {
        let event = resume_fallback_event(&TaskId("t-resume-fallback".to_string()));
        assert_eq!(event.event_kind, EventKind::Milestone);
        assert_eq!(event.detail, RESUME_FALLBACK_DETAIL);
    }

    #[test]
    fn finds_only_full_session_id_rollout_matches_in_nested_sessions_dir() {
        let temp = tempdir().unwrap();
        let sessions = temp.path().join("sessions/2026/08/09");
        fs::create_dir_all(&sessions).unwrap();
        let session_id = "019e3e49-6b83-7563-a3d8-b51a3a716dd1";
        fs::write(
            sessions.join(format!("rollout-2026-08-09T17-20-31-{session_id}.jsonl")),
            "{}",
        )
        .unwrap();
        fs::write(
            sessions.join("rollout-2026-08-09T17-20-31-extra-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl"),
            "{}",
        )
        .unwrap();
        fs::write(sessions.join("rollout-2026-08-09T17-20-31-session-123.jsonl"), "{}").unwrap();

        assert!(session_rollout_exists(
            temp.path().join("sessions").as_path(),
            session_id
        ));
        assert!(!session_rollout_exists(
            temp.path().join("sessions").as_path(),
            "123"
        ));
        assert!(session_rollout_exists(
            temp.path().join("sessions").as_path(),
            "session-123"
        ));
    }

    #[test]
    fn sandboxed_resume_skips_host_rollout_precheck() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: true,
            context_files: vec![],
            session_id: Some("not-a-uuid-session".to_string()),
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent
            .build_command_with_context(
                "continue",
                &opts,
                CommandContext {
                    durable_codex_home: false,
                    cargo_target_dir: None,
                },
            )
            .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(&args[..3], ["exec", "resume", "--json"]);
        assert!(args.contains(&"not-a-uuid-session".to_string()));
    }

    #[test]
    fn rollout_match_requires_exact_timestamp_boundary_and_session_id() {
        let path = Path::new(
            "rollout-2026-08-09T17-20-31-extra-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl",
        );
        assert!(!rollout_filename_matches(
            path,
            "019e3e49-6b83-7563-a3d8-b51a3a716dd1"
        ));
        assert!(!rollout_filename_matches(
            Path::new("rollout-2026-08-09T17-20-31-long-123.jsonl"),
            "123"
        ));
        assert!(rollout_filename_matches(
            Path::new("rollout-2026-08-09T17-20-31-session-123.jsonl"),
            "session-123"
        ));
        assert!(!rollout_filename_matches(
            Path::new("rollout-not-a-timestamp-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl"),
            "019e3e49-6b83-7563-a3d8-b51a3a716dd1"
        ));
    }

    #[test]
    fn parses_file_change_events() {
        let agent = CodexAgent;
        let line = r#"{"type":"item.completed","item":{"id":"item_5","type":"file_change","changes":[{"path":"/tmp/test.txt","kind":"update"}],"status":"completed"}}"#;
        let event = agent
            .parse_event(&TaskId("t-file".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::FileWrite);
        assert!(event.detail.contains("test.txt"));
    }

    #[test]
    fn parses_item_error_events() {
        let agent = CodexAgent;
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Model metadata for `o3` not found."}}"#;
        let event = agent
            .parse_event(&TaskId("t-err".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Error);
        assert!(event.detail.contains("Model metadata"));
    }

    #[test]
    fn parses_turn_completed_usage_metadata() {
        let agent = CodexAgent;
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":232452,"cached_input_tokens":211968,"output_tokens":5988}}"#;
        let event = agent
            .parse_event(&TaskId("t-usage".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Completion);
        assert_eq!(
            event
                .metadata
                .unwrap()
                .get("tokens")
                .and_then(|v| v.as_i64()),
            Some(238440)
        );
    }

    #[test]
    fn build_command_includes_skip_git_repo_check() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"--skip-git-repo-check".to_string()));
    }

    #[test]
    fn build_command_adds_worktree_metadata_to_writable_roots() {
        let temp = tempdir().unwrap();
        let worktree = temp.path().join("worktree");
        let common = temp.path().join("common/.git");
        let metadata = common.join("worktrees/bar");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&common).unwrap();
        fs::create_dir_all(&metadata).unwrap();
        fs::write(worktree.join(".git"), "gitdir: ../common/.git/worktrees/bar\n").unwrap();
        fs::write(metadata.join("commondir"), "../..\n").unwrap();
        let metadata = metadata.canonicalize().unwrap();
        let common = common.canonicalize().unwrap();
        let opts = RunOpts {
            dir: Some(worktree.to_string_lossy().to_string()),
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-c".to_string()));
        let expected = format!(
            "sandbox_workspace_write.writable_roots={}",
            toml::Value::Array(vec![
                toml::Value::String(metadata.to_string_lossy().to_string()),
                toml::Value::String(common.to_string_lossy().to_string()),
            ])
        );
        assert!(args.contains(&expected));
    }

    #[test]
    fn build_command_falls_back_when_commondir_missing() {
        let temp = tempdir().unwrap();
        let worktree = temp.path().join("worktree");
        let metadata = temp.path().join("foo/.git/worktrees/bar");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&metadata).unwrap();
        fs::write(worktree.join(".git"), "gitdir: ../foo/.git/worktrees/bar\n").unwrap();
        let metadata = metadata.canonicalize().unwrap();
        let opts = RunOpts {
            dir: Some(worktree.to_string_lossy().to_string()),
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-c".to_string()));
        let expected = format!(
            "sandbox_workspace_write.writable_roots={}",
            toml::Value::Array(vec![toml::Value::String(metadata.to_string_lossy().to_string())])
        );
        assert!(args.contains(&expected));
    }

    #[test]
    fn build_command_skips_writable_roots_for_regular_repo() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let opts = RunOpts {
            dir: Some(repo.to_string_lossy().to_string()),
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(!args.iter().any(|arg| {
            arg.starts_with("sandbox_workspace_write.writable_roots=")
        }));
    }

    #[test]
    fn build_command_handles_missing_gitfile_gracefully() {
        let temp = tempdir().unwrap();
        let opts = RunOpts {
            dir: Some(temp.path().to_string_lossy().to_string()),
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(!args.iter().any(|arg| {
            arg.starts_with("sandbox_workspace_write.writable_roots=")
        }));
    }

    #[test]
    fn build_command_read_only_omits_legacy_approval_flags() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: true,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(!args.contains(&"-s".to_string()));
        assert!(!args.contains(&"read-only".to_string()));
    }

    #[test]
    fn build_command_read_only_prepends_readonly_prefix() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: Some("result.md".to_string()),
            model: None,
            budget: false,
            read_only: true,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("READ-ONLY MODE"));
        assert!(last_arg.starts_with("IMPORTANT: READ-ONLY MODE"));
        assert!(last_arg.contains("EXCEPT the result file specified in this prompt"));
        assert!(last_arg.contains("analyze this code"));
    }

    #[test]
    fn build_command_read_only_without_result_file_keeps_strict_prefix() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: true,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("Do NOT modify, create, or delete any files. Only read and analyze."));
    }

    #[test]
    fn build_command_includes_context_files_in_prompt() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec!["Cargo.toml".to_string()],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("[Context File:"));
    }

    #[test]
    fn build_command_starts_fresh_when_saved_rollout_is_missing() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: Some("019e3e49-6b83-7563-a3d8-b51a3a716dd1".to_string()),
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("write the final report", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        // The approval flag sits between the fixed prefix and the prompt only when the
        // installed Codex version could be read, so pin the prefix and the prompt by
        // position from each end rather than assuming the flag is present.
        assert_eq!(&args[..3], ["exec", "--json", "--skip-git-repo-check"]);
        assert!(args.last().unwrap().contains("write the final report"));
    }
}

#[cfg(test)]
mod codex_nudge_tests;
