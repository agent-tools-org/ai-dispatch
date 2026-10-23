// Narrow event query regressions for metadata selection, timestamp ties, and liveness.
// Uses noisy histories and malformed metadata, comparing selectors to full-history references.

use super::*;
use crate::types::EventKind;

fn seed(store: &Store) {
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-read', 'codex', 'prompt', 'running', '2026-10-01T00:00:00Z')",
            [],
        )
        .unwrap();
    for i in 0..200 {
        insert(
            store,
            "reasoning",
            &format!("noise-{i}"),
            Some(&format!(r#"{{"payload":"{}"}}"#, "x".repeat(4096))),
        );
    }
}

fn insert(store: &Store, kind: &str, detail: &str, metadata: Option<&str>) {
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         VALUES ('t-read', '2026-10-01T00:00:00Z', ?1, ?2, ?3)",
            params![kind, detail, metadata],
        )
        .unwrap();
}

#[test]
fn completion_lookup_preserves_latest_usage_selection_and_skips_irrelevant_history() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    for metadata in [
        Some(r#"{"tokens":9,"cost_usd":1}"#),
        Some(r#"{"tokens":10,"cost_usd":2}"#),
        Some("{malformed"),
        Some("{}"),
        None,
    ] {
        insert(&store, "completion", "complete", metadata);
    }
    let expected = store
        .get_events("t-read")
        .unwrap()
        .into_iter()
        .rev()
        .filter(|event| event.event_kind == EventKind::Completion)
        .filter_map(|event| event.metadata)
        .find(|value| value.get("tokens").is_some() || value.get("cost_usd").is_some());
    assert_eq!(store.latest_usage_metadata("t-read").unwrap(), expected);
    store
        .db()
        .execute(
            "UPDATE events SET detail = X'ff' WHERE event_type = 'reasoning'",
            [],
        )
        .unwrap();
    assert!(store.get_events("t-read").is_err());
    assert_eq!(store.latest_usage_metadata("t-read").unwrap(), expected);
}

#[test]
fn completion_lookup_searches_the_existing_task_kind_index() {
    let store = Store::open_memory().unwrap();
    let conn = store.db();
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {COMPLETION_SQL}"))
        .unwrap();
    let plan = stmt
        .query_map(params!["t-read"], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH events USING INDEX idx_events_task_kind"),
        "{plan}"
    );
    assert!(plan.contains("event_type=?"), "{plan}");
    assert!(COMPLETION_SQL.contains("ORDER BY timestamp DESC, id DESC"));
}

fn seed_reports(store: &Store) {
    for (detail, metadata) in [
        (
            "infra-old",
            r#"{"verify_execution_error":true,"awaiting_prompt":"old"}"#,
        ),
        (
            "infra-new",
            r#"{"verify_execution_error":true,"awaiting_prompt":"new"}"#,
        ),
        (
            "wrong types",
            r#"{"verify_execution_error":1,"awaiting_prompt":42}"#,
        ),
        (
            "duplicate keys",
            r#"{"verify_execution_error":false,"verify_execution_error":true,
                "awaiting_prompt":false,"awaiting_prompt":"last duplicate"}"#,
        ),
        ("malformed", "{broken"),
    ] {
        insert(store, "error", detail, Some(metadata));
    }
}

#[test]
fn wait_and_infrastructure_reads_match_reverse_history_with_malformed_metadata() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    seed_reports(&store);
    let events = store.get_events("t-read").unwrap();
    let prompt = events
        .iter()
        .rev()
        .find_map(|event| {
            event
                .metadata
                .as_ref()
                .and_then(|m| m.get("awaiting_prompt"))
                .and_then(Value::as_str)
        })
        .map(str::to_string);
    let infra = events
        .iter()
        .rev()
        .find(|event| {
            event
                .metadata
                .as_ref()
                .is_some_and(|m| m["verify_execution_error"] == true)
        })
        .map(|e| e.detail.clone());
    assert_eq!(store.latest_waiting_prompt("t-read").unwrap(), prompt);
    assert_eq!(store.latest_infrastructure_error("t-read").unwrap(), infra);
    assert_eq!(store.latest_waiting_prompt("missing").unwrap(), None);
    assert_eq!(store.latest_infrastructure_error("missing").unwrap(), None);
}

#[test]
fn liveness_summary_matches_full_history_and_ignores_only_bookkeeping() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    for metadata in [
        r#"{"idle_warn":true}"#,
        r#"{"auto_escalated":true}"#,
        r#"{"acked_reply":true}"#,
        r#"{"source":"unstick-auto"}"#,
        r#"{"steered":true}"#,
        r#"{"input_delivery":null}"#,
        r#"{"message_id":null}"#,
        r#"{"idle_warn":true,"idle_warn":false}"#,
        r#"{"source":"unstick-auto","source":"agent"}"#,
        r#"{"idle_warn":1}"#,
        "{malformed",
    ] {
        insert(&store, "reasoning", "activity", Some(metadata));
    }
    insert(
        &store,
        "reasoning",
        &format!(
            "\u{2003}{}\u{2003}",
            crate::task::unstick::default_nudge_message()
        ),
        None,
    );
    insert(&store, "setup", "setup", None);
    insert(&store, "noop", "noop", None);
    let events = store.get_events("t-read").unwrap();
    let progress = events
        .iter()
        .filter(|event| is_progress(event))
        .collect::<Vec<_>>();
    let (count, last) = store.liveness_summary("t-read").unwrap();
    assert_eq!(count as usize, progress.len());
    assert_eq!(
        last,
        progress
            .last()
            .map(|event| (event.timestamp, event.detail.clone()))
    );
    assert_eq!(store.liveness_summary("missing").unwrap(), (0, None));
}

fn is_progress(event: &crate::types::TaskEvent) -> bool {
    if !event.event_kind.is_liveness() || event.event_kind == EventKind::Setup {
        return false;
    }
    if event.event_kind == EventKind::Reasoning
        && event.detail.trim() == crate::task::unstick::default_nudge_message()
    {
        return false;
    }
    let Some(m) = &event.metadata else {
        return true;
    };
    !["idle_warn", "auto_escalated", "acked_reply", "steered"]
        .iter()
        .any(|key| m.get(key).and_then(Value::as_bool) == Some(true))
        && m.get("source").and_then(Value::as_str) != Some("unstick-auto")
        && m.get("input_delivery").is_none()
        && m.get("message_id").is_none()
}

#[test]
fn reaper_counts_activity_when_serde_rejects_sqlite_valid_metadata() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    insert(
        &store,
        "reasoning",
        "live activity",
        Some(r#"{"idle_warn":true,"x":1e400}"#),
    );
    let events = store.get_events("t-read").unwrap();
    assert!(events.last().unwrap().metadata.is_none());
    let expected = events.iter().filter(|event| is_progress(event)).count();
    let (count, latest) = store.liveness_summary("t-read").unwrap();
    assert_eq!(count as usize, expected);
    assert_eq!(latest.unwrap().1, "live activity");
}

#[test]
fn metadata_prefilter_preserves_serde_rejection_and_escaped_keys() {
    let store = Store::open_memory().unwrap();
    seed(&store);
    insert(
        &store,
        "error",
        "valid infra",
        Some(r#"{"awaiting_prompt":"valid prompt","verify_execution_error":true}"#),
    );
    insert(
        &store,
        "error",
        "invalid infra",
        Some(r#"{"awaiting_prompt":"invalid prompt","verify_execution_error":true,"x":1e400}"#),
    );
    assert_eq!(
        store.latest_waiting_prompt("t-read").unwrap().as_deref(),
        Some("valid prompt")
    );
    assert_eq!(
        store.latest_infrastructure_error("t-read").unwrap().as_deref(),
        Some("valid infra")
    );
    insert(
        &store,
        "error",
        "escaped infra",
        Some(r#"{"awaiting_\u0070rompt":"escaped prompt","verify_execution_\u0065rror":true}"#),
    );
    assert_eq!(
        store.latest_waiting_prompt("t-read").unwrap().as_deref(),
        Some("escaped prompt")
    );
    assert_eq!(
        store.latest_infrastructure_error("t-read").unwrap().as_deref(),
        Some("escaped infra")
    );
}
