// GitButler integration: mode parsing, `but` CLI detection, helpers.
// Used by: project config, dispatch flow, merge workflows.
// Deps: anyhow, serde_json, std::{env, path, process, sync::OnceLock}.

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

static BUT_AVAILABLE: OnceLock<bool> = OnceLock::new();
static MAIN_REPO_PROJECT_CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

thread_local! {
    static AUTO_NO_PROJECT_NOTICE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ALWAYS_NO_PROJECT_NOTICE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static AUTO_SETUP_HINT_NOTICE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn set_once(notice: &'static std::thread::LocalKey<std::cell::Cell<bool>>) -> bool {
    notice.with(|cell| {
        if cell.get() {
            false
        } else {
            cell.set(true);
            true
        }
    })
}
const CLAUDE_TOOL_MATCHER: &str = "Edit|MultiEdit|Write";
const CLAUDE_SETTINGS_PATH: &str = ".claude/settings.local.json";

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TaskWorktreeIntegrationPlan {
    pub install_claude_hooks: bool,
    pub on_done_command: Option<String>,
    pub emit_setup_hint: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Off,
    Auto,
    Always,
}

impl Mode {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            other => bail!("unknown gitbutler mode '{other}'"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Always => "always",
        }
    }
}
/// Returns true iff the `but` binary is on PATH and `but --version` succeeds.
/// Result is cached for the process lifetime via OnceLock in production;
/// in test mode the check is evaluated on every call so tests can toggle
/// `AID_GITBUTLER_TEST_PRESENT` without the first call poisoning the cache.
pub fn but_available() -> bool {
    #[cfg(test)]
    {
        return detect_but_available();
    }
    #[cfg(not(test))]
    *BUT_AVAILABLE.get_or_init(detect_but_available)
}
/// Resolves whether GitButler features should run for this dispatch.
/// `Always` => true. `Auto` => true iff `but_available()`. `Off` => false.
pub fn is_active(mode: Mode) -> bool {
    match mode {
        Mode::Off => false,
        Mode::Auto => but_available(),
        Mode::Always => true,
    }
}
/// Run `but setup` in the main repo and ignore "already set up" failures.
pub fn ensure_setup(repo_dir: &Path) -> Result<()> {
    let output = Command::new("but").arg("setup").current_dir(repo_dir).output()?;
    if output.status.success() || setup_already_done(&output) {
        return Ok(());
    }
    bail!("{}", command_failure_message("but setup", &output));
}

/// Run `but apply <branch>` inside `repo_dir`.
pub fn apply_branch(repo_dir: &Path, branch: &str) -> Result<()> {
    if !but_available() {
        bail!("GitButler CLI not found. Install: https://gitbutler.com");
    }
    let output = Command::new("but")
        .arg("apply")
        .arg(branch)
        .current_dir(repo_dir)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    bail!("{}", command_failure_message(&format!("but apply {branch}"), &output));
}

/// Returns true when the agent uses Claude Code hooks for lifecycle automation.
pub fn agent_uses_claude_hooks(agent_kind: &str) -> bool {
    matches!(agent_kind.to_ascii_lowercase().as_str(), "claude" | "claude-code")
}

/// Install per-worktree Claude Code hooks for GitButler.
pub fn install_claude_hooks(worktree: &Path) -> Result<()> {
    let settings_path = worktree.join(CLAUDE_SETTINGS_PATH);
    let mut root = read_settings_json(&settings_path)?;
    let hooks = ensure_object_field(&mut root, "hooks")?;
    upsert_hook(hooks, "PreToolUse", Some(CLAUDE_TOOL_MATCHER), "but claude pre-tool");
    upsert_hook(hooks, "PostToolUse", Some(CLAUDE_TOOL_MATCHER), "but claude post-tool");
    upsert_hook(hooks, "Stop", None, "but claude stop");
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(settings_path, serde_json::to_vec_pretty(&root)?)?;
    Ok(())
}

/// Returns the completion command for non-Claude agents in GitButler mode.
pub fn on_done_command(worktree: &Path) -> String {
    format!(
        "but -C {} commit -i || true",
        shell_quote(&worktree.to_string_lossy())
    )
}

pub(crate) fn task_worktree_integration_plan(
    repo_dir: &Path,
    worktree: &Path,
    mode: Mode,
    agent_kind: &str,
) -> TaskWorktreeIntegrationPlan {
    if !is_active(mode) {
        return TaskWorktreeIntegrationPlan::default();
    }

    if !main_repo_has_project(repo_dir) {
        match mode {
            Mode::Auto => {
                if set_once(&AUTO_NO_PROJECT_NOTICE) {
                    aid_warn!(
                        "[aid] gitbutler = auto but main repo has no GitButler project — skipping per-task GitButler hooks"
                    );
                }
                return TaskWorktreeIntegrationPlan {
                    emit_setup_hint: set_once(&AUTO_SETUP_HINT_NOTICE),
                    ..Default::default()
                };
            }
            Mode::Always => {
                if set_once(&ALWAYS_NO_PROJECT_NOTICE) {
                    aid_warn!(
                        "[aid] gitbutler = always but main repo has no GitButler project — skipping per-task GitButler hooks"
                    );
                }
                return TaskWorktreeIntegrationPlan::default();
            }
            Mode::Off => return TaskWorktreeIntegrationPlan::default(),
        }
    }

    if agent_uses_claude_hooks(agent_kind) {
        return TaskWorktreeIntegrationPlan { install_claude_hooks: true, ..Default::default() };
    }
    TaskWorktreeIntegrationPlan {
        on_done_command: Some(on_done_command(worktree)),
        ..Default::default()
    }
}

pub(crate) fn main_repo_has_project(repo_dir: &Path) -> bool {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AID_GITBUTLER_TEST_PROJECT_PRESENT") {
        return matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    }

    if !but_available() {
        return false;
    }

    let key = repo_dir.canonicalize().unwrap_or_else(|_| repo_dir.to_path_buf()).to_string_lossy().to_string();
    let cache = MAIN_REPO_PROJECT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(status) = cache.lock().ok().and_then(|cache| cache.get(&key).copied()) {
        return status;
    }

    let status = Command::new("but")
        .args(["status", "--json"])
        .current_dir(repo_dir)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, status);
    }
    status
}

fn detect_but_available() -> bool {
    #[cfg(test)]
    {
        return std::env::var("AID_GITBUTLER_TEST_PRESENT")
            .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
    }

    #[cfg(not(test))]
    Command::new("but")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn setup_already_done(output: &std::process::Output) -> bool {
    let message = command_failure_message("but setup", output).to_ascii_lowercase();
    message.contains("already set up") || message.contains("already setup")
}

fn command_failure_message(command: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => format!("{command} failed with status {}", output.status),
        (false, true) => format!("{command} failed: {stdout}"),
        (true, false) => format!("{command} failed: {stderr}"),
        (false, false) => format!("{command} failed: {stderr} ({stdout})"),
    }
}

fn read_settings_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let value = serde_json::from_slice::<Value>(&std::fs::read(path)?)?;
    if value.is_object() {
        Ok(value)
    } else {
        bail!("{} must contain a JSON object", path.display());
    }
}

fn ensure_object_field<'a>(root: &'a mut Value, field: &str) -> Result<&'a mut Map<String, Value>> {
    let Some(root_object) = root.as_object_mut() else {
        bail!("settings root must be a JSON object");
    };
    let entry = root_object
        .entry(field.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    match entry.as_object_mut() {
        Some(object) => Ok(object),
        None => bail!("settings field '{field}' must be a JSON object"),
    }
}

fn upsert_hook(
    hooks: &mut Map<String, Value>,
    event_name: &str,
    matcher: Option<&str>,
    command: &str,
) {
    let entry = hooks
        .entry(event_name.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !entry.is_array() {
        *entry = Value::Array(Vec::new());
    }
    if let Some(items) = entry.as_array_mut() {
        let index = items.iter().position(|value| matcher_matches(value, matcher));
        let hook_value = build_hook_value(matcher, command);
        if let Some(index) = index {
            items[index] = hook_value;
        } else {
            items.push(hook_value);
        }
    }
}

fn matcher_matches(value: &Value, matcher: Option<&str>) -> bool {
    let current = value.get("matcher").and_then(Value::as_str).unwrap_or("");
    match matcher {
        Some(expected) => current == expected,
        None => current.is_empty(),
    }
}

fn build_hook_value(matcher: Option<&str>, command: &str) -> Value {
    match matcher {
        Some(matcher) => json!({
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}],
        }),
        None => json!({
            "hooks": [{"type": "command", "command": command}],
        }),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests;
