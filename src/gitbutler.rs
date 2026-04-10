// GitButler integration: mode parsing, `but` CLI detection, helpers.
// Used by: project config, dispatch flow, merge workflows.
// Deps: anyhow, serde_json, std::{env, path, process, sync::OnceLock}.

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

static BUT_AVAILABLE: OnceLock<bool> = OnceLock::new();
const CLAUDE_TOOL_MATCHER: &str = "Edit|MultiEdit|Write";
const CLAUDE_SETTINGS_PATH: &str = ".claude/settings.local.json";
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
/// Result is cached for the process lifetime via OnceLock.
pub fn but_available() -> bool {
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

/// Run `but setup` in the worktree and ignore "already set up" failures.
pub fn ensure_setup(worktree: &Path) -> Result<()> {
    let output = Command::new("but")
        .arg("setup")
        .current_dir(worktree)
        .output()?;
    if output.status.success() || setup_already_done(&output) {
        return Ok(());
    }
    bail!("{}", command_failure_message("but setup", &output));
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

fn detect_but_available() -> bool {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AID_GITBUTLER_TEST_PRESENT") {
        return matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    }

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
mod tests {
    use super::{Mode, agent_uses_claude_hooks, but_available, install_claude_hooks, is_active, on_done_command};
    use serde_json::{Value, json};
    use std::fs;

    #[test]
    fn gitbutler_mode_parse_round_trip() {
        for expected in [Mode::Off, Mode::Auto, Mode::Always] {
            let parsed = Mode::from_str(expected.as_str()).unwrap();
            assert_eq!(parsed, expected);
        }
    }

    #[test]
    fn gitbutler_mode_rejects_unknown_value() {
        assert!(Mode::from_str("sometimes").is_err());
    }

    #[test]
    fn gitbutler_is_active_is_false_for_off() {
        assert!(!is_active(Mode::Off));
    }

    #[test]
    fn gitbutler_is_active_is_true_for_always() {
        assert!(is_active(Mode::Always));
    }

    #[test]
    #[ignore = "process-wide cache; enable when explicitly validating detection"]
    fn gitbutler_but_available_respects_test_override() {
        unsafe {
            std::env::set_var("AID_GITBUTLER_TEST_PRESENT", "1");
        }
        assert!(but_available());
    }

    #[test]
    fn agent_uses_claude_hooks_matches_known_agents() {
        assert!(agent_uses_claude_hooks("claude"));
        assert!(agent_uses_claude_hooks("claude-code"));
        assert!(!agent_uses_claude_hooks("codex"));
        assert!(!agent_uses_claude_hooks("cursor"));
        assert!(!agent_uses_claude_hooks("opencode"));
        assert!(!agent_uses_claude_hooks("gemini"));
    }

    #[test]
    fn install_claude_hooks_writes_expected_settings_json() {
        let temp = tempfile::tempdir().unwrap();
        install_claude_hooks(temp.path()).unwrap();
        let value: Value =
            serde_json::from_slice(&fs::read(temp.path().join(".claude/settings.local.json")).unwrap()).unwrap();
        assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"].as_str(), Some("Edit|MultiEdit|Write"));
        assert_eq!(value["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str(), Some("but claude pre-tool"));
        assert_eq!(value["hooks"]["PostToolUse"][0]["matcher"].as_str(), Some("Edit|MultiEdit|Write"));
        assert_eq!(value["hooks"]["PostToolUse"][0]["hooks"][0]["command"].as_str(), Some("but claude post-tool"));
        assert_eq!(value["hooks"]["Stop"][0]["hooks"][0]["command"].as_str(), Some("but claude stop"));
    }

    #[test]
    fn install_claude_hooks_preserves_existing_settings_keys() {
        let temp = tempfile::tempdir().unwrap();
        let settings_dir = temp.path().join(".claude");
        fs::create_dir_all(&settings_dir).unwrap();
        fs::write(
            settings_dir.join("settings.local.json"),
            serde_json::to_vec_pretty(&json!({
                "theme": "dark",
                "hooks": {
                    "Notification": [{
                        "hooks": [{"type": "command", "command": "echo notify"}]
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        install_claude_hooks(temp.path()).unwrap();

        let value: Value =
            serde_json::from_slice(&fs::read(temp.path().join(".claude/settings.local.json")).unwrap()).unwrap();
        assert_eq!(value["theme"].as_str(), Some("dark"));
        assert_eq!(value["hooks"]["Notification"][0]["hooks"][0]["command"].as_str(), Some("echo notify"));
        assert_eq!(value["hooks"]["Stop"][0]["hooks"][0]["command"].as_str(), Some("but claude stop"));
    }

    #[test]
    fn on_done_command_contains_gitbutler_commit_shell_command() {
        let temp = tempfile::tempdir().unwrap();
        let command = on_done_command(temp.path());
        let worktree = temp.path().to_string_lossy();

        assert!(command.contains("but -C"));
        assert!(command.contains(worktree.as_ref()));
        assert!(command.contains("commit -i"));
        assert!(command.contains("|| true"));
    }
}
