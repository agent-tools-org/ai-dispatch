// Advise output for served-but-unrated models newer than the advised model.
// Deps: advise(), unrated_served_line, served-model disk cache, test codex home.

use super::*;

#[test]
fn advise_surfaces_newer_unrated_served_codex_model() {
    let home = tempfile::tempdir().expect("temp aid home");
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::paths::ensure_dirs().expect("aid dirs");
    let codex_home = home.path().join("codex");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    std::fs::write(codex_home.join("config.toml"), "model = \"gpt-5.6-sol\"\n").expect("config");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex_home));
    let now = chrono::Utc::now().timestamp();
    let cache = serde_json::json!({"codex": {"models": ["gpt-6-sol"], "updated_at_secs": now}});
    std::fs::write(crate::paths::aid_dir().join("served_models_cache.json"), cache.to_string())
        .expect("served-model cache");
    let declared = DeclaredTaskProfile {
        difficulty: crate::types::TaskDifficulty::Complex,
        budget: crate::types::TaskBudget::Premium,
        urgency: crate::types::TaskUrgency::Normal,
        rigor: crate::types::TaskRigor::Standard,
    };
    let report = advise("Implement the parser", declared, None, None, None, 20, None);
    crate::agent::codex::cli_config::set_test_codex_home(None);
    let codex = report.candidates.iter().find(|c| c.agent == "codex").expect("codex");
    assert_eq!(codex.model.as_deref(), Some("gpt-5.6-sol"), "unrated model is not auto-selected");
    assert_eq!(codex.source, crate::agent::run_model::RunModelSource::CliConfig);
    assert_eq!(codex.unrated_served_models, vec!["gpt-6-sol".to_string()]);
    let line = unrated_served_line(codex).expect("human line");
    assert!(line.contains("gpt-6-sol") && line.contains("gpt-5.6-sol"), "{line}");
}
