// Hermetic binary-level coverage for default Codex route persistence and display.
// Uses the delivery fixture and explicitly separates HOME from CODEX_HOME.
use super::*;

#[test]
fn default_route_uses_isolated_codex_home_and_show_custom_agent_is_null() {
    let aid_home = TempDir::new().unwrap();
    let host_home = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    write_fake_codex(bin.path());
    let codex = host_home.path().join(".codex");
    std::fs::create_dir(&codex).unwrap();
    std::fs::write(codex.join("config.toml"), "model_provider='host-provider'").unwrap();
    let output = aid_cmd_in(aid_home.path())
        .env("HOME", host_home.path())
        .env("PATH", test_path(bin.path()))
        .env("FAKE_CODEX_SHORT_FINAL", "1")
        .args(["run", "codex", "Reply ok", "--id", "t-default-route"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let show = aid_cmd_in(aid_home.path())
        .args(["show", "t-default-route", "--json"])
        .output()
        .unwrap();
    assert!(show.status.success());
    let task: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(task["agent"], "codex");
    assert_eq!(task["custom_agent"], serde_json::Value::Null);
    let cost = aid_cmd_in(aid_home.path())
        .args(["cost", "--agent", "codex", "--period", "all"])
        .output()
        .unwrap();
    assert!(
        cost.status.success(),
        "{}",
        String::from_utf8_lossy(&cost.stderr)
    );
    assert!(String::from_utf8_lossy(&cost.stdout).contains("codex/openai-chatgpt-plan/"));
}
