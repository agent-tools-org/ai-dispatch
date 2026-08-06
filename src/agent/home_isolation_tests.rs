use super::*;
use std::fs;
use std::process::Command;

#[test]
fn isolation_negative_control() {
    let temp = tempfile::tempdir().unwrap();
    let mock_real_home = temp.path().join("mock_home");
    let mock_claude_dir = mock_real_home.join(".claude");
    fs::create_dir_all(&mock_claude_dir).unwrap();
    let marker_path = mock_claude_dir.join("CLAUDE.md");
    let marker_content = "SECRET_ORCHESTRATOR_INSTRUCTION_MARKER_998877";
    fs::write(&marker_path, marker_content).unwrap();

    let gitconfig_path = mock_real_home.join(".gitconfig");
    fs::write(&gitconfig_path, "[user]\nname = Test User\n").unwrap();

    // 1. Negative control check without isolation:
    assert!(marker_path.exists());
    assert_eq!(fs::read_to_string(&marker_path).unwrap(), marker_content);

    // 2. Positive check with isolation:
    let guard = IsolatedHomeGuard::create_from_home(Some(&mock_real_home), None).unwrap();

    // The instruction file and .claude directory MUST be absent in the isolated HOME.
    assert!(!guard.path().join(".claude/CLAUDE.md").exists());
    assert!(!guard.path().join(".claude").exists());

    // Non-denylisted entry (.gitconfig) MUST be present in the isolated HOME.
    assert!(guard.path().join(".gitconfig").exists());
    assert_eq!(fs::read_to_string(guard.path().join(".gitconfig")).unwrap(), "[user]\nname = Test User\n");
}

#[test]
fn cargo_and_git_work_in_isolated_home() {
    let guard = IsolatedHomeGuard::create(None).unwrap();

    // cargo --version must succeed (exit code 0) inside isolated home.
    let cargo_output = Command::new("cargo")
        .arg("--version")
        .env("HOME", guard.path())
        .output()
        .expect("cargo should be runnable");
    assert!(cargo_output.status.success(), "cargo --version failed under isolated HOME: {}", String::from_utf8_lossy(&cargo_output.stderr));

    // git config or commit must succeed inside isolated home.
    let git_output = Command::new("git")
        .args(["config", "--global", "--get", "user.name"])
        .env("HOME", guard.path())
        .output()
        .expect("git should be runnable");
    assert!(git_output.status.success(), "git config failed under isolated HOME: {}", String::from_utf8_lossy(&git_output.stderr));
}

#[test]
fn isolated_home_cleanup_on_drop() {
    let iso_path = {
        let guard = IsolatedHomeGuard::create(None).unwrap();
        let path = guard.path().to_path_buf();
        assert!(path.exists());
        path
    };
    assert!(!iso_path.exists(), "Isolated HOME directory should be cleaned up on drop");
}

#[test]
fn denylist_contains_claude_identity_surfaces() {
    assert!(DEFAULT_DENYLIST.contains(&".claude"));
    assert!(DEFAULT_DENYLIST.contains(&".claude.json"));
}
