// GitButler integration: mode parsing, `but` CLI detection, helpers.
// Used by: project config, dispatch flow, merge workflows.
// Deps: anyhow, serde_json, std::{env, path, process, sync::OnceLock}.

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

mod hooks;
mod mode;
mod repo;
use hooks::{ensure_object_field, read_settings_json, upsert_hook};
pub use mode::Mode;

#[cfg(not(test))]
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

pub(crate) use repo::repo_has_markers;
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
    let but_cmd = get_but_command();
    let output = Command::new(&but_cmd).arg("setup").current_dir(repo_dir).output()?;
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
    let but_cmd = get_but_command();
    let output = Command::new(&but_cmd)
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
    upsert_hook(
        hooks,
        "PreToolUse",
        Some(CLAUDE_TOOL_MATCHER),
        "but claude pre-tool",
    );
    upsert_hook(
        hooks,
        "PostToolUse",
        Some(CLAUDE_TOOL_MATCHER),
        "but claude post-tool",
    );
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

#[cfg(test)]
thread_local! {
    static TEST_BUT_AVAILABLE: std::cell::RefCell<Option<bool>> = const { std::cell::RefCell::new(None) };
    static TEST_PROJECT_PRESENT: std::cell::RefCell<Option<bool>> = const { std::cell::RefCell::new(None) };
    static TEST_BUT_COMMAND: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_but_available(available: Option<bool>) {
    TEST_BUT_AVAILABLE.with(|cell| *cell.borrow_mut() = available);
}

#[cfg(test)]
pub fn set_test_project_present(present: Option<bool>) {
    TEST_PROJECT_PRESENT.with(|cell| *cell.borrow_mut() = present);
}

#[cfg(test)]
pub fn set_test_but_command(command: Option<String>) {
    TEST_BUT_COMMAND.with(|cell| *cell.borrow_mut() = command);
}

fn get_but_command() -> String {
    #[cfg(test)]
    {
        if let Some(cmd) = TEST_BUT_COMMAND.with(|cell| cell.borrow().clone()) {
            return cmd;
        }
    }
    "but".to_string()
}

pub(crate) fn main_repo_has_project(repo_dir: &Path) -> bool {
    #[cfg(test)]
    {
        let override_val = TEST_PROJECT_PRESENT.with(|cell| *cell.borrow());
        if let Some(val) = override_val {
            return val;
        }
    }

    if !but_available() {
        return false;
    }

    let key = repo_dir.canonicalize().unwrap_or_else(|_| repo_dir.to_path_buf()).to_string_lossy().to_string();
    let cache = MAIN_REPO_PROJECT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(status) = cache.lock().ok().and_then(|cache| cache.get(&key).copied()) {
        return status;
    }

    let but_cmd = get_but_command();
    let status = Command::new(&but_cmd)
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
        let override_val = TEST_BUT_AVAILABLE.with(|cell| *cell.borrow());
        if let Some(val) = override_val {
            return val;
        }
    }

    let but_cmd = get_but_command();
    Command::new(&but_cmd)
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests;
