// Tests for GitButler integration helpers and CLI command assembly.
// Deps: super, serde_json, tempfile.

use super::{
    Mode, agent_uses_claude_hooks, apply_branch, but_available, install_claude_hooks, is_active,
    on_done_command,
};
use serde_json::{Value, json};
use std::fs;

fn gitbutler_test_present() -> bool {
    std::env::var("AID_GITBUTLER_TEST_PRESENT")
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

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
fn apply_branch_errors_when_test_but_detection_is_disabled() {
    if gitbutler_test_present() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let error = apply_branch(temp.path(), "lane-branch").unwrap_err().to_string();
    assert_eq!(error, "GitButler CLI not found. Install: https://gitbutler.com");
}

#[test]
fn apply_branch_real_execution_requires_test_override() {
    if !gitbutler_test_present() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let result = apply_branch(temp.path(), "lane-branch");
    assert!(result.is_err());
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
