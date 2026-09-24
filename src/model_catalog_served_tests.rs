// Tests served-only catalog rows, family/version comparison, and the probe-agent list.
// Exports: module-scoped tests only.
// Deps: model_catalog_served, isolated AID home, served-model disk cache.

use super::*;

fn write_served_cache(agent: &str, models: &[&str]) {
    crate::paths::ensure_dirs().expect("aid dirs");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let cache = serde_json::json!({ agent: {"models": models, "updated_at_secs": now} });
    std::fs::write(
        crate::paths::aid_dir().join("served_models_cache.json"),
        cache.to_string(),
    )
    .expect("served-model cache");
}

#[test]
fn family_version_splits_family_and_numeric_version() {
    assert_eq!(family_version("gpt-5.6-sol"), Some(("gpt", vec![5, 6])));
    assert_eq!(family_version("gpt-6-sol"), Some(("gpt", vec![6])));
    assert_eq!(
        family_version("claude-opus-5"),
        Some(("claude-opus", vec![5]))
    );
    assert_eq!(
        family_version("composer-2.5"),
        Some(("composer", vec![2, 5]))
    );
    assert_eq!(family_version("coder-model"), None);
    assert_eq!(family_version("5-only"), None);
}

#[test]
fn served_only_rows_are_unrated_and_skip_catalog_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    write_served_cache("codex", &["gpt-5.6-sol", "gpt-6-sol"]);
    let rows = served_only_models(AgentKind::Codex);
    assert_eq!(rows.len(), 1, "catalog row gpt-5.6-sol is not repeated");
    let row = &rows[0];
    assert_eq!(row.model, "gpt-6-sol");
    assert_eq!(
        (row.capability, row.input_per_m, row.output_per_m),
        (None, None, None)
    );
    assert_eq!(row.origin, ModelOrigin::Served);
}

#[test]
fn unrated_newer_models_are_same_family_and_higher_version_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    write_served_cache(
        "codex",
        &[
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-4.1",
            "o9-mini",
            "codex-auto",
        ],
    );
    assert_eq!(
        unrated_served_newer_than(AgentKind::Codex, "gpt-5.6-sol"),
        vec!["gpt-6-sol".to_string(), "gpt-6-luna".to_string()]
    );
    assert!(unrated_served_newer_than(AgentKind::Codex, "gpt-7").is_empty());
    assert!(unrated_served_newer_than(AgentKind::Codex, "auto").is_empty());
}

#[test]
fn missing_cache_yields_no_served_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    assert!(served_only_models(AgentKind::Codex).is_empty());
}

#[test]
fn agents_outside_probe_list_use_the_default_served_models() {
    for kind in AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .chain([AgentKind::Claude])
    {
        if SERVED_PROBE_AGENTS.contains(&kind) {
            continue;
        }
        let served = crate::agent::get_agent(kind)
            .served_models()
            .expect("served models");
        assert!(
            served.is_none(),
            "{kind:?} probes served models; add it to SERVED_PROBE_AGENTS"
        );
    }
}
