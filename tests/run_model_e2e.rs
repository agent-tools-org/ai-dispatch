// `aid run` and `aid advise` resolve the same model from the codex home.
// Covers the CLI-config default (reported, never pinned) and the served-model probe.
// Deps: compiled aid binary, isolated AID_HOME and CODEX_HOME, serde_json, tempfile.

mod common;
use common::aid_cmd_in;
use tempfile::TempDir;

const PROFILE: &[&str] = &[
    "--difficulty", "moderate", "--budget", "standard", "--urgency", "normal", "--rigor", "standard",
];

fn codex_home(config_model: Option<&str>) -> TempDir {
    let home = TempDir::new().expect("codex home");
    if let Some(model) = config_model {
        std::fs::write(home.path().join("config.toml"), format!("model = \"{model}\"\n"))
            .expect("codex config");
    }
    home
}

fn advise_codex(codex: &TempDir) -> serde_json::Value {
    let aid_home = TempDir::new().expect("aid home");
    let output = aid_cmd_in(aid_home.path()).env("CODEX_HOME", codex.path())
        .args(["advise", "Refactor the scheduler", "--kind", "refactoring", "--json", "--top", "0"])
        .args(PROFILE).output().expect("run advise");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("advice JSON");
    report["candidates"].as_array().expect("candidates").iter()
        .find(|item| item["agent"] == "codex").expect("codex candidate").clone()
}

#[test]
fn advise_reports_the_codex_cli_default_unpinned() {
    let codex = advise_codex(&codex_home(Some("gpt-6-sol")));
    assert_eq!(codex["model"], "gpt-6-sol");
    assert_eq!(codex["pinned"], false);
    assert_eq!(codex["source"], "cli_config");
}

#[test]
fn advise_without_a_readable_cli_default_names_no_model() {
    let codex = advise_codex(&codex_home(None));
    assert!(codex["model"].is_null(), "{codex}");
    assert_eq!(codex["pinned"], false);
    assert_eq!(codex["source"], "agent_default");
}

#[test]
fn run_reports_the_same_cli_default_without_passing_it() {
    let codex = codex_home(Some("gpt-6-sol"));
    let aid_home = TempDir::new().expect("aid home");
    let output = aid_cmd_in(aid_home.path()).env("CODEX_HOME", codex.path())
        .args(["run", "codex", "Refactor validation", "--no-hint", "--no-skill", "--dry-run"])
        .args(PROFILE).output().expect("run preview");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("codex model: gpt-6-sol; source: CLI config (no -m)"), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("gpt-6-sol"), "an unpinned default is never passed: {stdout}");
}

#[test]
fn served_model_probe_reads_models_cache_from_codex_home() {
    let codex = codex_home(None);
    std::fs::write(codex.path().join("models_cache.json"), r#"{"models":[{"slug":"gpt-6-sol"}]}"#)
        .expect("models cache");
    let aid_home = TempDir::new().expect("aid home");
    let output = aid_cmd_in(aid_home.path()).env("CODEX_HOME", codex.path())
        .args(["run", "codex", "Refactor validation", "--no-hint", "--no-skill", "--dry-run"])
        .args(["--model", "not-a-served-model"]).args(PROFILE).output().expect("run preview");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("does not serve model 'not-a-served-model'"), "{stderr}");
    assert!(stderr.contains("Served models: gpt-6-sol"), "{stderr}");
}
