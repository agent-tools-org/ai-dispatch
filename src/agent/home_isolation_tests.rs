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

    let mock_agents_dir = mock_real_home.join(".agents").join("skills").join("secret-skill");
    fs::create_dir_all(&mock_agents_dir).unwrap();
    let skill_marker = mock_agents_dir.join("SKILL.md");
    fs::write(&skill_marker, "SECRET_SKILL").unwrap();

    let gitconfig_path = mock_real_home.join(".gitconfig");
    fs::write(&gitconfig_path, "[user]\nname = Test User\n").unwrap();

    // 1. Negative control check without isolation:
    assert!(marker_path.exists());
    assert_eq!(fs::read_to_string(&marker_path).unwrap(), marker_content);
    assert!(skill_marker.exists());

    // 2. Positive check with isolation:
    let guard = IsolatedHomeGuard::create_from_home(Some(&mock_real_home), None).unwrap();

    // The instruction file and .claude directory MUST be absent in the isolated HOME.
    assert!(!guard.path().join(".claude/CLAUDE.md").exists());
    assert!(!guard.path().join(".claude").exists());

    // The shared .agents directory MUST be absent in the isolated HOME.
    assert!(!guard.path().join(".agents").exists());

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
fn denylist_contains_claude_and_agents_identity_surfaces() {
    assert!(DEFAULT_DENYLIST.contains(&".claude"));
    assert!(DEFAULT_DENYLIST.contains(&".claude.json"));
    assert!(DEFAULT_DENYLIST.contains(&".anthropic"));
    assert!(DEFAULT_DENYLIST.contains(&".agents"));
    assert!(DEFAULT_DENYLIST.contains(&".agent"));
}

#[test]
fn build_isolated_home_fails_without_real_home() {
    match IsolatedHomeGuard::create_from_home(None, None) {
        Ok(_) => panic!("expected error when real home is unknown"),
        Err(err) => assert!(
            err.to_string().contains("real home directory is unknown"),
            "unexpected error: {err:#}"
        ),
    }
}

#[test]
fn build_isolated_home_fails_when_real_home_not_directory() {
    let temp = tempfile::tempdir().unwrap();
    let not_a_dir = temp.path().join("not-a-dir");
    fs::write(&not_a_dir, "file").unwrap();
    match IsolatedHomeGuard::create_from_home(Some(&not_a_dir), None) {
        Ok(_) => panic!("expected error when real home is not a directory"),
        Err(err) => assert!(
            err.to_string().contains("is not a directory"),
            "unexpected error: {err:#}"
        ),
    }
}

#[test]
fn build_isolated_home_fails_when_real_home_unreadable() {
    let temp = tempfile::tempdir().unwrap();
    let real_home = temp.path().join("home");
    fs::create_dir(&real_home).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&real_home).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&real_home, perms).unwrap();
        match IsolatedHomeGuard::create_from_home(Some(&real_home), None) {
            Ok(_) => panic!("expected error when real home is unreadable"),
            Err(err) => assert!(
                err.to_string().contains("cannot read real HOME"),
                "unexpected error: {err:#}"
            ),
        }
        let mut perms = fs::metadata(&real_home).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&real_home, perms).unwrap();
    }
}
