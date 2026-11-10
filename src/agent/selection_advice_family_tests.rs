// Family candidate regressions over real advise wiring and isolated caches.
// Covers live-model precedence, independent pools, and explicit launch pins.
use super::*;
use serde_json::json;

fn report(claude_used: f64, models: &[&str]) -> AdviceReport {
    let now = chrono::Utc::now();
    std::fs::write(
        crate::support::paths::aid_dir().join("served_models_cache.json"),
        json!({"agy": {"models": models, "updated_at_secs": now.timestamp()}}).to_string(),
    )
    .expect("served cache");
    let reset = (now + chrono::Duration::hours(5)).to_rfc3339();
    let windows = [
        ("gemini-4-argon", 100.0),
        ("gemini", 0.0),
        ("claude-gpt", claude_used),
    ]
    .map(|(group, used)| {
        json!({"label": format!("{group} Weekly"),
            "group": group, "used_percent": used, "resets_at": reset})
    });
    std::fs::write(
        crate::support::paths::aid_dir().join("aidbar/agy.json"),
        json!({"ok": true, "snapshot": {"provider": "agy", "windows": windows,
            "fetched_at": now.to_rfc3339()}})
        .to_string(),
    )
    .expect("quota cache");
    advise(
        "refactor",
        tests::declared(crate::types::TaskDifficulty::Moderate, TaskBudget::Standard),
        None,
        None,
        None,
        0,
        None,
    )
}

fn candidate<'a>(report: &'a AdviceReport, model: &str) -> &'a AdviceCandidate {
    report
        .candidates
        .iter()
        .find(|c| c.agent == "agy" && c.model.as_deref() == Some(model))
        .expect("pinned family candidate")
}

#[test]
fn agy_family_headroom_is_independent() {
    let (_temp, _home, _cache) = tests::isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Antigravity]);
    let report = report(
        0.0,
        &[
            "claude-opus-5-5-low",
            "claude-opus-5-5-high",
            "gemini-4-argon-high",
            "gemini-3.8-flash-high",
        ],
    );
    let claude = candidate(&report, "claude-opus-5-5-high");
    assert!(claude.pinned);
    assert_eq!(claude.source, RunModelSource::Explicit);
    assert_eq!(claude.breakdown.headroom_penalty, 0.0);
    assert_eq!(claude.quota.used_percent, Some(0.0));
    assert_eq!(claude.quota.status, "dispatchable");
    assert_eq!(
        candidate(&report, "gemini-4-argon-high")
            .breakdown
            .headroom_penalty,
        -6.0
    );
}

#[test]
fn agy_spent_claude_pool_keeps_penalty() {
    let (_temp, _home, _cache) = tests::isolated();
    let report = report(100.0, &["claude-opus-5-5-high"]);
    assert_eq!(
        candidate(&report, "claude-opus-5-5-high")
            .breakdown
            .headroom_penalty,
        -6.0
    );
}

#[test]
fn agy_served_cache_replaces_catalog_candidates() {
    let (_temp, _home, _cache) = tests::isolated();
    let report = report(0.0, &["claude-sonnet-5-5-medium"]);
    let models: Vec<_> = report
        .candidates
        .iter()
        .filter(|c| c.agent == "agy")
        .map(|c| c.model.as_deref())
        .collect();
    assert_eq!(models, vec![Some("claude-sonnet-5-5-medium")]);
}

#[test]
fn agy_history_does_not_cross_families_or_use_unobserved_requests() {
    let (_temp, _home, _cache) = tests::isolated();
    let store = Store::open_memory().expect("store");
    for index in 0..5 {
        let id = format!("t-family-{index}");
        store
            .insert_waiting_task(
                &id, "agy", "test", None, None, None, None, None, None, false, false,
            )
            .expect("task");
        store
            .db()
            .execute(
                "UPDATE tasks SET status = 'failed',
            observed_model = 'gemini-4-argon-high' WHERE id = ?1",
                [&id],
            )
            .expect("history");
    }
    let _ = report(0.0, &["claude-opus-5-5-high", "gemini-4-argon-high"]);
    let advice = advise(
        "refactor",
        tests::declared(crate::types::TaskDifficulty::Moderate, TaskBudget::Standard),
        None,
        None,
        Some(&store),
        0,
        None,
    );
    assert_eq!(
        candidate(&advice, "claude-opus-5-5-high")
            .breakdown
            .history_bonus,
        0.0
    );
    assert_eq!(
        candidate(&advice, "gemini-4-argon-high")
            .breakdown
            .history_bonus,
        -5.0
    );
    store
        .db()
        .execute(
            "UPDATE tasks SET requested_model = observed_model,
        observed_model = NULL",
            [],
        )
        .expect("unattributed");
    let advice = advise(
        "refactor",
        tests::declared(crate::types::TaskDifficulty::Moderate, TaskBudget::Standard),
        None,
        None,
        Some(&store),
        0,
        None,
    );
    assert_eq!(
        candidate(&advice, "gemini-4-argon-high")
            .breakdown
            .history_bonus,
        0.0
    );
}

#[test]
fn agy_catalog_fallback_has_current_family_pins() {
    let (_temp, _home, _cache) = tests::isolated();
    let models = family::models(AgentKind::Antigravity, TaskCategory::Refactoring);
    for expected in [
        "claude-opus-5-5-high",
        "gemini-4-argon-high",
        "gemini-3.8-flash-high",
    ] {
        assert!(
            models
                .iter()
                .any(|model| model.as_deref() == Some(expected))
        );
    }
    assert_eq!(models.len(), 3);
}
