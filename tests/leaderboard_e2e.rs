// Third-party leaderboard evidence through real advice and agent-list commands.
// Uses the captured sample and synthetic price aliases; never fetches network data.
// Deps: compiled aid, isolated command helper, serde_json, tempfile.

mod common;
use common::aid_cmd_in;
use serde_json::{Value, json};
use tempfile::TempDir;

fn seeded() -> TempDir {
    let home = TempDir::new().expect("home");
    let mut feed: Value =
        serde_json::from_str(include_str!("fixtures/scores-sample.json")).expect("sample");
    feed["built_at"] = json!(chrono::Utc::now().to_rfc3339());
    std::fs::write(home.path().join("scores.json"), feed.to_string()).expect("scores");
    let prices = json!({"built_at": chrono::Utc::now().to_rfc3339(), "age_seconds": 0,
    "stale": false, "models": [
        {"id": "302ai/kimi-k2-thinking", "aliases": ["gpt-5.6-sol"],
         "input_per_mtok": 1.0, "output_per_mtok": 1.0},
        {"id": "302ai/qwen3-30b-a3b", "aliases": [],
         "input_per_mtok": 1.0, "output_per_mtok": 1.0}
    ]});
    std::fs::write(home.path().join("prices.json"), prices.to_string()).expect("prices");
    std::fs::write(
        home.path().join("agent_config.toml"),
        "[codex]\nmodel = 'gpt-5.6-sol'\n",
    )
    .expect("sticky");
    home
}

fn advice(home: &TempDir, json: bool) -> std::process::Output {
    let mut cmd = aid_cmd_in(home.path());
    cmd.env("XDG_CACHE_HOME", home.path().join("quota")).args([
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
    assert_eq!(evidence["canonical_id"], "302ai/kimi-k2-thinking");
    assert_eq!(evidence["capability"], 10.0);
    assert_eq!(evidence["harness"], "harness unmeasured");
    let score = &evidence["scores"][0];
    assert_eq!(score["source"], "epoch");
    assert_eq!(score["value"], 146.01);
    assert_eq!(score["rank"], 79);
    assert_eq!(score["of"], 274);
    assert!(score["effort"].is_null());
    assert_eq!(score["date"], "2025-11-06");
}

#[test]
fn leaderboard_advice_json_reports_raw_values_and_attribution() {
    let home = seeded();
    let payload: Value = serde_json::from_slice(&advice(&home, true).stdout).expect("JSON");
    let candidates = payload["candidates"].as_array().expect("candidates");
    let codex = candidates
        .iter()
        .find(|row| row["agent"] == "codex")
        .expect("codex");
    assert_evidence(&codex["capability_evidence"]);
    assert!(
        candidates
            .iter()
            .any(|row| row["capability_evidence"]["capability"].is_null())
    );
    assert!(codex["breakdown"].get("complexity_bonus").is_none());
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
        text.contains("epoch / eci: 146.01 index rank 79/274 effort unknown"),
        "{text}"
    );
    assert!(text.contains("date 2025-11-06"));
    assert!(text.contains("capability: unknown"));
    assert!(text.contains("harness unmeasured"));
    assert!(text.contains("Source epoch: CC BY 4.0 | Epoch AI | https://epoch.ai/"));
}

#[test]
fn leaderboard_agent_list_json_uses_same_evidence_and_unknown_is_null() {
    let home = seeded();
    let output = aid_cmd_in(home.path())
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
    let codex = agents
        .iter()
        .find(|agent| agent["name"] == "codex")
        .expect("codex");
    let rows = codex["models"]["available"].as_array().expect("models");
    let model = rows
        .iter()
        .find(|row| row["model"] == "gpt-5.6-sol")
        .expect("model");
    assert_evidence(&model["capability_evidence"]);
    assert_eq!(model["capability"], 10.0);
    assert_eq!(model["rated"], true);
    let unknown = rows
        .iter()
        .find(|row| row["model"] == "gpt-5.4-mini")
        .expect("unknown model");
    assert!(unknown["capability"].is_null());
    assert_eq!(unknown["rated"], false);
    assert_eq!(payload["sources"]["epoch"]["licence"], "CC BY 4.0");
}
