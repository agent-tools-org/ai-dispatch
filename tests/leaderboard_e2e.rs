// Third-party leaderboard evidence through real advice and agent-list commands.
// Uses captured relay scores and price aliases; never fetches network data.
// Deps: compiled aid, isolated command helper, serde_json, tempfile.

mod common;
use common::aid_cmd_in;
use serde_json::{Value, json};
use tempfile::TempDir;

fn seeded() -> TempDir {
    let home = TempDir::new().expect("home");
    let mut feed: Value = serde_json::from_str(include_str!(
        "fixtures/leaderboard/scores-trimmed-20261009.json"
    ))
    .expect("scores capture");
    feed["built_at"] = json!(chrono::Utc::now().to_rfc3339());
    feed["age_seconds"] = json!(0);
    std::fs::write(home.path().join("scores.json"), feed.to_string()).expect("scores");
    let mut prices: Value = serde_json::from_str(include_str!(
        "fixtures/leaderboard/prices-trimmed-20261009.json"
    ))
    .expect("prices capture");
    prices["built_at"] = json!(chrono::Utc::now().to_rfc3339());
    std::fs::write(home.path().join("prices.json"), prices.to_string()).expect("prices");
    std::fs::write(
        home.path().join("agent_config.toml"),
        "[claude]\nmodel = 'opus'\n",
    )
    .expect("sticky");
    home
}

fn advice(home: &TempDir, json: bool) -> std::process::Output {
    let mut cmd = aid_cmd_in(home.path());
    cmd.env("CLAUDE_CODE_EFFORT_LEVEL", "")
        .env("XDG_CACHE_HOME", home.path().join("quota"))
        .args([
            "advise",
            "Compare benchmark results",
            "--kind",
            "research",
            "--difficulty",
            "moderate",
            "--budget",
            "standard",
            "--urgency",
            "normal",
            "--rigor",
            "standard",
            "--top",
            "0",
        ]);
    if json {
        cmd.arg("--json");
    }
    let output = cmd.output().expect("advice");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn assert_evidence(evidence: &Value) {
    assert_eq!(evidence["canonical_id"], "anthropic/claude-opus-5.5");
    assert_eq!(evidence["capability"], 10.0);
    assert_eq!(evidence["harness"], "harness unmeasured");
    let score = evidence["scores"]
        .as_array()
        .expect("scores")
        .iter()
        .find(|score| score["source"] == "epoch" && score["board"] == "eci")
        .expect("ECI");
    assert_eq!(score["source"], "epoch");
    assert_eq!(score["value"], 167.33);
    assert_eq!(score["rank"], 1);
    assert_eq!(score["of"], 274);
    assert!(score["effort"].is_null());
    assert_eq!(score["date"], "2026-09-22");
}

#[test]
fn leaderboard_advice_json_reports_raw_values_and_attribution() {
    let home = seeded();
    let payload: Value = serde_json::from_slice(&advice(&home, true).stdout).expect("JSON");
    let candidates = payload["candidates"].as_array().expect("candidates");
    let claude = candidates
        .iter()
        .find(|row| row["agent"] == "claude")
        .expect("claude");
    assert_evidence(&claude["capability_evidence"]);
    assert!(
        candidates
            .iter()
            .any(|row| row["capability_evidence"]["capability"].is_null())
    );
    assert!(claude["breakdown"].get("complexity_bonus").is_none());
    assert_eq!(payload["sources"]["epoch"]["licence"], "CC BY 4.0");
    assert_eq!(payload["sources"]["epoch"]["attribution"], "Epoch AI");
    assert_eq!(
        payload["sources"]["epoch"]["url"],
        "https://epoch.ai/data/benchmark_data.zip"
    );
}

#[test]
fn leaderboard_advice_text_names_raw_source_rank_effort_date_and_unknown() {
    let home = seeded();
    let text = String::from_utf8(advice(&home, false).stdout).expect("text");
    assert!(
        text.contains("epoch / eci: 167.33 index rank 1/274 effort unknown"),
        "{text}"
    );
    assert!(text.contains("date 2026-09-22"));
    assert!(text.contains("capability: unknown"));
    assert!(text.contains("harness unmeasured"));
    assert!(text.contains("Source epoch: CC BY 4.0 | Epoch AI | https://epoch.ai/"));
}

#[test]
fn leaderboard_agent_list_json_uses_same_evidence_and_unknown_is_null() {
    let home = seeded();
    let output = aid_cmd_in(home.path())
        .env("CLAUDE_CODE_EFFORT_LEVEL", "")
        .args(["agent", "list", "--json"])
        .output()
        .expect("list");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let agents = payload["agents"].as_array().expect("agents");
    let claude = agents
        .iter()
        .find(|agent| agent["name"] == "claude")
        .expect("claude");
    let rows = claude["models"]["available"].as_array().expect("models");
    let model = rows
        .iter()
        .find(|row| row["model"] == "opus")
        .expect("model");
    assert_evidence(&model["capability_evidence"]);
    assert_eq!(model["capability"], 10.0);
    assert_eq!(model["rated"], true);
    let unknown = rows
        .iter()
        .find(|row| row["model"] == "haiku")
        .expect("unknown model");
    assert!(unknown["capability"].is_null());
    assert_eq!(unknown["rated"], false);
    assert_eq!(payload["sources"]["epoch"]["licence"], "CC BY 4.0");
}
