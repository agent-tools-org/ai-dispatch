// Exact advice pins distinguish definitive served evidence from unknown refreshes.
// Covers live mismatch, stale cache refresh failure and empty served responses.
// Deps: parent queryable agent fixture and model validation cache.
use super::*;

#[test]
fn advised_pin_allows_unknown_refresh_and_refuses_definitive_mismatch() {
    let _lock = lock_test();
    let home = tempfile::tempdir().expect("home");
    let _home = crate::paths::AidHomeGuard::set(home.path());
    for fresh in [None, Some(Vec::new()), Some(vec!["different-model".into()])] {
        clear_served_models_cache();
        let kind = AgentKind::Kilo;
        cache().lock().expect("cache").insert(
            kind,
            CachedServedModels {
                models: Some(vec!["cached-model".into()]),
                from_live_probe: false,
                fingerprint: None,
            },
        );
        let definitive = fresh.as_ref().is_some_and(|models| !models.is_empty());
        let agent = MockQueryableAgent::new(kind, fresh);
        let result = validate_model_for_agent(&agent, "exact-advice-pin", ModelSource::Advised);
        if definitive {
            assert!(
                result
                    .expect_err("definitive mismatch")
                    .to_string()
                    .contains("refusing a different default")
            );
        } else {
            assert!(result.expect("unknown evidence permits the exact pin"));
        }
    }
}
