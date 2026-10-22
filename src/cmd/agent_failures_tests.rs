// Marker-only prompt, grouping, filtering and credential-redaction regressions.
// Uses a memory store without invoking a provider.
use super::*;
use crate::{support::paths::AidHomeGuard, types::AgentKind};

fn insert(store: &Store, kind: &str, excerpt: &str, age: i64, agent: &str) {
    let data = serde_json::json!({"unclassified_failure": true,
        "agent": agent, "provider_excerpt": excerpt});
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
        VALUES ('t-failure', ?1, ?2, 'DISTINCTIVE_EVENT_DETAIL', ?3)",
            rusqlite::params![
                (Local::now() - chrono::Duration::days(age)).to_rfc3339(),
                kind,
                data.to_string()
            ],
        )
        .expect("event");
}

#[test]
fn marker_only_prompt_groups_identical_excerpts_and_filters() {
    let store = Store::open_memory().expect("store");
    insert(&store, "milestone", "connection closed", 1, "codex");
    insert(&store, "milestone", "connection closed", 2, "agy");
    insert(&store, "error", "DISTINCTIVE_OTHER_EVENT", 0, "codex");
    insert(&store, "milestone", "DISTINCTIVE_OLD_EVENT", 10, "codex");
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
        VALUES ('t-failure', ?1, 'milestone', 'private', ?2)",
            rusqlite::params![
                Local::now().to_rfc3339(),
                r#"{"provider_excerpt":"DISTINCTIVE_UNMARKED_EVENT"}"#
            ],
        )
        .expect("unmarked");
    let cutoff = Local::now() - duration("7d").expect("duration");
    let groups = load(&store, None, cutoff, 50).expect("groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].count, 2);
    assert_eq!(groups[0].agents.len(), 2);
    let prompt = review_prompt(&groups).expect("prompt");
    assert!(prompt.contains("connection closed"));
    assert!(!prompt.contains("DISTINCTIVE"));
    assert!(!prompt.contains("t-failure"));
    assert_eq!(
        load(&store, Some("codex"), cutoff, 50).expect("filtered")[0].count,
        1
    );
    assert_eq!(load(&store, None, cutoff, 1).expect("limited")[0].count, 1);
    assert!(load(&store, None, cutoff, 0).expect("zero").is_empty());
}

#[test]
fn secrets_are_redacted_in_json_and_review() {
    let store = Store::open_memory().expect("store");
    let values = [
        "synthetic-bearer-value",
        "sk-synthetic-example",
        "synthetic-api-value",
        "synthetic-key-value",
        "synthetic-token-value",
    ];
    let excerpt = format!(
        "Bearer {} {} api_key={} key={} token={}",
        values[0], values[1], values[2], values[3], values[4]
    );
    insert(&store, "milestone", &excerpt, 0, "codex");
    let groups = load(
        &store,
        None,
        Local::now() - duration("7d").expect("duration"),
        50,
    )
    .expect("groups");
    for output in [
        serde_json::to_string(&groups).expect("json"),
        review_prompt(&groups).expect("prompt"),
    ] {
        for value in values {
            assert!(!output.contains(value));
        }
        assert!(output.contains("[REDACTED]"));
    }
}

#[tokio::test]
async fn no_markers_skips_model_and_preserves_rate_limit_marker() {
    let temp = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(temp.path());
    let path = crate::routing::rate_limit::marker_path(&AgentKind::Codex, None);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(&path, "synthetic marker state").expect("marker");
    let before = std::fs::read(&path).expect("before");
    run(
        Arc::new(Store::open_memory().expect("store")),
        None,
        "7d",
        50,
        false,
        Some("nonexistent-model-agent".into()),
    )
    .await
    .expect("empty review");
    assert_eq!(std::fs::read(&path).expect("after"), before);
}

#[test]
fn duration_rejects_invalid_and_overflow() {
    for value in ["", "7", "-1d", "999999999999999999999d", "7x"] {
        assert!(duration(value).is_err(), "{value}");
    }
    assert_eq!(duration("7d").expect("week"), chrono::Duration::days(7));
}

#[tokio::test]
async fn populated_review_is_blocked_without_mutating_markers() {
    let temp = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(temp.path());
    let path = crate::routing::rate_limit::marker_path(&AgentKind::Codex, None);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(&path, "synthetic marker state").expect("marker");
    let before = std::fs::read(&path).expect("before");
    let store = Arc::new(Store::open_memory().expect("store"));
    insert(&store, "milestone", "connection closed", 0, "codex");
    let error = run(store, None, "7d", 50, false, Some(String::new()))
        .await
        .expect_err("unsafe model path must not execute");
    assert!(error.to_string().contains("unavailable"));
    assert_eq!(std::fs::read(&path).expect("after"), before);
}
