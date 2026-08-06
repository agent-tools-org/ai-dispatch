// End-to-end coverage for the read-only `aid advise` command.
// Verifies JSON output, zero store writes, and success under fleet-wide limits.
// Deps: compiled aid binary, isolated AID_HOME, serde_json, tempfile.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

const PROFILE_ARGS: &[&str] = &[
    "--difficulty", "moderate",
    "--budget", "standard",
    "--urgency", "normal",
    "--rigor", "standard",
    "--json",
    "--top", "0",
];

#[test]
fn advise_emits_json_without_creating_store_state() {
    let aid_home = TempDir::new().expect("temp AID_HOME");
    let before = directory_entries(aid_home.path());
    let output = aid_cmd_in(aid_home.path())
        .args(["advise", "Refactor src/main.rs safely"])
        .args(PROFILE_ARGS)
        .output()
        .expect("run advise");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).expect("advice JSON");
    assert_eq!(payload["declared"]["difficulty"], "moderate");
    assert!(payload["recommended"].is_object());
    assert!(payload["candidates"].as_array().is_some_and(|items| !items.is_empty()));
    assert_eq!(directory_entries(aid_home.path()), before);
    assert!(!aid_home.path().join("aid.db").exists());
}

#[test]
fn advise_ranks_actionable_builtins_before_separate_custom_context() {
    let aid_home = TempDir::new().expect("temp AID_HOME");
    let agents_dir = aid_home.path().join("agents");
    std::fs::create_dir_all(&agents_dir).expect("create agents directory");
    std::fs::write(
        agents_dir.join("researcher.toml"),
        "[agent]\nid = \"researcher\"\ndisplay_name = \"Researcher\"\ncommand = \"true\"\n[agent.capabilities]\nresearch = 15\nsimple_edit = 15\n",
    ).expect("write custom agent");
    let output = aid_cmd_in(aid_home.path())
        .args(["advise", "add a null check to the parser"])
        .args(PROFILE_ARGS)
        .output()
        .expect("run advise");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).expect("advice JSON");
    assert_eq!(payload["inferred"]["kind"], "simple-edit");
    let candidates = payload["candidates"].as_array().expect("built-in candidates");
    let recommended = payload["recommended"]["agent"].as_str().expect("recommended agent");
    assert_eq!(candidates[0]["agent"], recommended);
    // Soft eligibility: floor/budget shortfalls are ranking penalties with reasons, not a hard gate.
    let with_reason = candidates.iter().find(|item| item["eligible"] == false);
    if let Some(item) = with_reason {
        assert!(item["exclusion_reason"].as_str().is_some_and(|text| !text.is_empty()));
    }
    assert!(candidates.iter().all(|item| item["agent"] != "researcher"));
    assert_eq!(payload["custom_candidates"][0]["agent"], "researcher");
    assert!(payload["custom_candidates"][0].get("score").is_none());
}

#[test]
fn advise_succeeds_when_every_builtin_is_rate_limited() {
    let aid_home = TempDir::new().expect("temp AID_HOME");
    for agent in [
        "gemini", "qwen", "codex", "copilot", "opencode", "commandcode",
        "cursor", "kilo", "mimocode", "codebuff", "droid", "oz", "claude",
        "agy", "grok",
    ] {
        std::fs::write(
            aid_home.path().join(format!("rate-limit-{agent}")),
            "recovery_at: Dec 31, 2099 11:59 PM\nmessage: quota exhausted\n",
        ).expect("write test marker");
    }
    let output = aid_cmd_in(aid_home.path())
        .args(["advise", "Implement a complex cross-file refactor"])
        .args(PROFILE_ARGS)
        .output()
        .expect("run advise");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).expect("advice JSON");
    let candidates = payload["candidates"].as_array().expect("candidate array");
    assert!(!candidates.is_empty());
    assert!(candidates.iter().all(|item| item["breakdown"]["rate_limit_penalty"] == -10.0));
    assert!(payload["recommended"].is_object());
    assert!(!aid_home.path().join("aid.db").exists());
}

#[test]
fn show_json_includes_persisted_declared_profile() {
    let aid_home = TempDir::new().expect("temp AID_HOME");
    let initialize = aid_cmd_in(aid_home.path()).arg("board").output().expect("initialize store");
    assert!(initialize.status.success());
    let connection = rusqlite::Connection::open(aid_home.path().join("aid.db")).expect("open store");
    connection.execute(
        "INSERT INTO tasks (
         id, agent, prompt, status, created_at, declared_difficulty, declared_budget,
         declared_urgency, declared_rigor
         ) VALUES (
         't-declared', 'codex', 'prompt', 'done', '2026-08-05T00:00:00Z',
         'complex', 'premium', 'urgent', 'critical'
         )",
        [],
    ).expect("insert task");
    drop(connection);

    let output = aid_cmd_in(aid_home.path())
        .args(["show", "t-declared", "--json"])
        .output()
        .expect("show task");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).expect("show JSON");
    assert_eq!(payload["declared"]["difficulty"], "complex");
    assert_eq!(payload["declared"]["budget"], "premium");
    assert_eq!(payload["declared"]["urgency"], "urgent");
    assert_eq!(payload["declared"]["rigor"], "critical");
}

fn directory_entries(path: &std::path::Path) -> Vec<String> {
    let mut entries = std::fs::read_dir(path).expect("read AID_HOME")
        .map(|entry| entry.expect("directory entry").file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}
