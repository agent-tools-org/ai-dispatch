// Backend label attribution regressions for agy.
// Uses captured model-table columns and diagnostic text.
use super::*;
#[test]
fn selected_backend_label_maps_to_exact_served_id() {
    let log = "Propagating selected model override to backend: label=\"Gemini 4 Argon (Medium)\"";
    let label = selected_label(log).expect("label");
    assert_eq!(
        model_for_label(&["gemini-4-argon-medium".into()], label),
        Some("gemini-4-argon-medium".into())
    );
    assert_eq!(
        model_for_label(&["gemini-4-argon-high".into()], label),
        None
    );
}

#[test]
fn agy_completion_maps_only_cached_served_models() {
    let home = crate::support::test_env::TestHome::with_dirs();
    let id = TaskId("t-observed-cache".into());
    let path = crate::support::paths::agent_log_path(id.as_str());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(
        path,
        "Propagating selected model override to backend: label=\"Gemini 4 Argon (Medium)\"",
    )
    .expect("log");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_secs();
    std::fs::write(
        home.path().join("served_models_cache.json"),
        serde_json::json!({
            "agy": {"models": ["gemini-4-argon-medium"], "updated_at_secs": now}
        })
        .to_string(),
    )
    .expect("cache");
    assert_eq!(
        observed_model(&id).as_deref(),
        Some("gemini-4-argon-medium")
    );
    std::fs::remove_file(home.path().join("served_models_cache.json")).expect("remove cache");
    assert_eq!(observed_model(&id), None);
}
