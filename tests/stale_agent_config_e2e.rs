// Stale removed-agent settings must not block inventory or supported dispatch.
// Checks real CLI warnings once per key and explicit dispatch rejection.
// Deps: compiled aid, isolated AID_HOME, shared subprocess helpers.
mod common;
use common::aid_cmd_in;

#[test]
fn stale_config_warns_once_per_key_without_disabling_supported_agents() {
    let home = tempfile::tempdir().expect("home");
    let path = home.path().join("agent_config.toml");
    let content = "[gemini]\nmodel = 'old'\n[qwen]\ndisabled = true\n\
        [codex]\nmodel = 'configured-codex'\n";
    std::fs::write(&path, content).expect("config");
    let empty_path = home.path().join("no-binaries");
    std::fs::create_dir(&empty_path).expect("empty PATH");
    let output = aid_cmd_in(home.path())
        .env("PATH", &empty_path)
        .args(["agent", "list", "--json"])
        .output()
        .expect("inventory");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for key in ["gemini"] {
        let warning = format!(
            "Ignoring stale removed-agent config key [{key}] in {}",
            path.display()
        );
        assert_eq!(stderr.matches(&warning).count(), 1, "{stderr}");
        let explicit = aid_cmd_in(home.path())
            .args(["run", key, "Review code", "--no-hint"])
            .output()
            .expect("explicit removed dispatch");
        assert!(!explicit.status.success());
        assert!(String::from_utf8_lossy(&explicit.stderr).contains("removed in this release"));
    }
    assert!(String::from_utf8_lossy(&output.stdout).contains("configured-codex"));
    assert!(!stderr.contains("config key [qwen]"));
    assert_eq!(
        std::fs::read_to_string(path).expect("unchanged config"),
        content
    );
}
