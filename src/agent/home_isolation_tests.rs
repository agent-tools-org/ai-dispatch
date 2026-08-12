// Tests for isolated HOME construction and stable host toolchain paths.
// Exports: none. Deps: home isolation guard, Cargo, tempfile.

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

    // The instruction file MUST be absent while .claude directory is present.
    assert!(!guard.path().join(".claude/CLAUDE.md").exists());
    assert!(guard.path().join(".claude").exists());

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

    // git must be usable inside the isolated home. Reading a pre-existing global
    // user.name would only assert that the host machine has one — an isolated HOME
    // legitimately has no gitconfig — so write one and read it back instead.
    let git_set = Command::new("git")
        .args(["config", "--global", "user.name", "Isolated Home"])
        .env("HOME", guard.path())
        .output()
        .expect("git should be runnable");
    assert!(git_set.status.success(), "git config write failed under isolated HOME: {}", String::from_utf8_lossy(&git_set.stderr));
    let git_output = Command::new("git")
        .args(["config", "--global", "--get", "user.name"])
        .env("HOME", guard.path())
        .output()
        .expect("git should be runnable");
    assert!(git_output.status.success(), "git config read failed under isolated HOME: {}", String::from_utf8_lossy(&git_output.stderr));
    assert_eq!(String::from_utf8_lossy(&git_output.stdout).trim(), "Isolated Home");
    assert!(guard.path().join(".gitconfig").exists(), "the write must land inside the isolated HOME, not the operator's");
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
fn denylist_contains_agents_identity_surfaces() {
    assert!(!DEFAULT_DENYLIST.contains(&".claude"));
    assert!(!DEFAULT_DENYLIST.contains(&".claude.json"));
    assert!(DEFAULT_DENYLIST.contains(&".anthropic"));
    assert!(DEFAULT_DENYLIST.contains(&".agents"));
    assert!(DEFAULT_DENYLIST.contains(&".agent"));
}

#[test]
fn claude_credential_reachability_and_instruction_masking_under_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let mock_home = temp.path().join("mock_home");

    // Auth credentials file in HOME
    let claude_json = mock_home.join(".claude.json");
    fs::create_dir_all(&mock_home).unwrap();
    fs::write(&claude_json, "{\"oauthAccount\": {\"email\": \"user@example.com\"}}").unwrap();

    // .claude directory mixing instructions and credentials/data
    let claude_dir = mock_home.join(".claude");
    fs::create_dir_all(claude_dir.join("sessions")).unwrap();
    fs::create_dir_all(claude_dir.join("skills").join("my-skill")).unwrap();
    fs::create_dir_all(claude_dir.join("agents").join("my-agent")).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), "# Orchestrator Secret Prompt").unwrap();
    fs::write(claude_dir.join("CLAUDE.md.bak"), "# Backup Prompt").unwrap();
    fs::write(claude_dir.join("settings.json"), "{\"permissions\": {}}").unwrap();
    fs::write(claude_dir.join("settings.local.json"), "{\"local\": true}").unwrap();
    fs::write(claude_dir.join("history.jsonl"), "{\"session\": \"123\"}").unwrap();
    fs::write(claude_dir.join("sessions").join("sess.json"), "{}").unwrap();

    let guard = IsolatedHomeGuard::create_from_home(Some(&mock_home), None).unwrap();
    let iso_path = guard.path();

    // 1. .claude.json auth file MUST be present and reachable
    assert!(iso_path.join(".claude.json").exists());
    assert_eq!(
        fs::read_to_string(iso_path.join(".claude.json")).unwrap(),
        "{\"oauthAccount\": {\"email\": \"user@example.com\"}}"
    );

    // 2. .claude directory MUST exist
    assert!(iso_path.join(".claude").is_dir());

    // 3. Instruction files and directories MUST be masked (absent)
    assert!(!iso_path.join(".claude/CLAUDE.md").exists());
    assert!(!iso_path.join(".claude/CLAUDE.md.bak").exists());
    assert!(!iso_path.join(".claude/settings.json").exists());
    assert!(!iso_path.join(".claude/settings.local.json").exists());
    assert!(!iso_path.join(".claude/skills").exists());
    assert!(!iso_path.join(".claude/agents").exists());

    // 4. Non-instruction runtime data (sessions, history) MUST be present
    assert!(iso_path.join(".claude/history.jsonl").exists());
    assert!(iso_path.join(".claude/sessions/sess.json").exists());
}

#[test]
fn aid_directory_reachability_under_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let mock_home = temp.path().join("mock_home");
    let aid_dir = mock_home.join(".aid");
    fs::create_dir_all(&aid_dir).unwrap();
    fs::write(aid_dir.join("credentials.toml"), "[auth]\ntoken = \"xyz\"\n").unwrap();

    let guard = IsolatedHomeGuard::create_from_home(Some(&mock_home), None).unwrap();
    let iso_path = guard.path();

    // .aid MUST be reachable in isolated home
    assert!(iso_path.join(".aid/credentials.toml").exists());
    assert_eq!(
        fs::read_to_string(iso_path.join(".aid/credentials.toml")).unwrap(),
        "[auth]\ntoken = \"xyz\"\n"
    );
}

#[test]
fn container_sandbox_home_interplay() {
    let guard = IsolatedHomeGuard::create(None).unwrap();
    let mut cmd = Command::new("claude");
    cmd.env("HOME", guard.path());

    // 1. Sandbox wrap_command test:
    let wrapped_sandbox = crate::sandbox::wrap_command(&cmd, "t-test", crate::types::AgentKind::Claude, false);
    let sandbox_args: Vec<String> = wrapped_sandbox
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    // Host HOME path must NOT be forwarded as env var
    let host_home_str = guard.path().to_string_lossy().into_owned();
    assert!(!sandbox_args.iter().any(|arg| arg.contains(&format!("HOME={host_home_str}"))));
    // Container HOME=/root must be set
    assert!(sandbox_args.iter().any(|arg| arg == "HOME=/root"));

    // 2. Container exec_in_container test:
    let wrapped_container = crate::container::exec_in_container(&cmd, "aid-container");
    let container_args: Vec<String> = wrapped_container
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    assert!(!container_args.iter().any(|arg| arg.contains(&format!("HOME={host_home_str}"))));
    assert!(container_args.iter().any(|arg| arg == "HOME=/root"));
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

#[cfg(unix)]
#[test]
fn cargo_build_uses_rustc_outside_isolated_home() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();

    let capture = temp.path().join("rustc-path");
    let wrapper = temp.path().join("rustc-wrapper.sh");
    fs::write(&wrapper, "#!/bin/sh\nprintf '%s' \"$1\" > \"$AID_RUSTC_CAPTURE\"\nrustc=\"$1\"\nshift\nexec \"$rustc\" \"$@\"\n").unwrap();
    let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions).unwrap();

    let guard = IsolatedHomeGuard::create(None).unwrap();
    let isolated_home = guard.path().to_path_buf();
    let mut cargo = Command::new("cargo");
    cargo
        .args(["build", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .env_remove("CARGO_HOME")
        .env_remove("RUSTUP_HOME")
        .env("HOME", guard.path())
        .env("RUSTC_WRAPPER", &wrapper)
        .env("AID_RUSTC_CAPTURE", &capture);
    guard.apply_toolchain_env(&mut cargo);

    let output = cargo.output().unwrap();
    assert!(output.status.success(), "cargo build failed: {}", String::from_utf8_lossy(&output.stderr));
    drop(guard);

    let rustc_path = fs::read_to_string(&capture).unwrap();
    assert!(!rustc_path.starts_with(&isolated_home.to_string_lossy().to_string()));
    assert!(Path::new(&rustc_path).is_file(), "rustc path did not survive HOME cleanup: {rustc_path}");
    assert!(!isolated_home.exists());
}
