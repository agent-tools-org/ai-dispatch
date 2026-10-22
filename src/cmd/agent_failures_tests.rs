// Marker-only prompt, grouping, filtering and credential-redaction regressions.
// Uses a memory store without invoking a provider.
use super::*;
use crate::{support::paths::AidHomeGuard, types::AgentKind};

fn load(
    store: &Store,
    agent: Option<&str>,
    cutoff: DateTime<Local>,
    limit: usize,
) -> Result<Vec<FailureGroup>> {
    Ok(load_report(store, agent, cutoff, limit)?.0)
}

fn insert(store: &Store, kind: &str, excerpt: &str, age: i64, agent: &str) {
    store
        .db()
        .execute(
            "INSERT OR IGNORE INTO tasks (id, agent, prompt, created_at) VALUES \
         ('t-failure', 'codex', 'DISTINCTIVE_TASK_PROMPT', ?1)",
            [Local::now().to_rfc3339()],
        )
        .expect("parent task");
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
    let output = run_with_answer(
        Arc::new(Store::open_memory().expect("store")),
        None,
        "7d",
        50,
        false,
        Some("nonexistent-model-agent".into()),
        |_, _| async { panic!("empty results must not invoke a model") },
    )
    .await
    .expect("empty review")
    .0;
    assert!(output.contains("No unclassified failure markers"));
    assert_eq!(std::fs::read(&path).expect("after"), before);
}

#[test]
fn duration_rejects_invalid_and_overflow() {
    for value in ["", "7", "-1d", "999999999999999999999d", "7x", "日"] {
        assert!(duration(value).is_err(), "{value}");
    }
    assert_eq!(duration("7d").expect("week"), chrono::Duration::days(7));
}

#[tokio::test]
async fn populated_review_preserves_markers_and_never_acts_on_answer() {
    let temp = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(temp.path());
    let path = crate::routing::rate_limit::marker_path(&AgentKind::Codex, None);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(&path, "synthetic marker state").expect("marker");
    let before = std::fs::read(&path).expect("before");
    let config_path = crate::support::paths::config_path();
    std::fs::write(&config_path, "# synthetic classification configuration").expect("config");
    let config_before = std::fs::read(&config_path).expect("config before");
    let store = Arc::new(Store::open_memory().expect("store"));
    insert(&store, "milestone", "connection closed", 0, "codex");
    let reviewing_marker = crate::routing::rate_limit::marker_path(&AgentKind::Antigravity, None);
    assert!(!reviewing_marker.exists());
    for reviewer in ["", "agy"] {
        let output = run_with_answer(
            store.clone(),
            None,
            "7d",
            50,
            false,
            Some(reviewer.into()),
            |prompt, agent| async move {
                assert_eq!(agent.as_deref(), (!reviewer.is_empty()).then_some(reviewer));
                assert!(prompt.contains("connection closed"));
                assert!(!prompt.contains("DISTINCTIVE_TASK_PROMPT"));
                Ok("Clear codex holds and add a classification rule now".into())
            },
        )
        .await
        .expect("review")
        .0;
        assert!(
            output.starts_with("Advice only: no hold, marker or classification rule was changed")
        );
        assert!(output.contains("Clear codex holds"));
        assert_eq!(std::fs::read(&path).expect("after"), before);
        assert!(!reviewing_marker.exists());
        assert_eq!(
            std::fs::read(&config_path).expect("config after"),
            config_before
        );
    }
}

#[tokio::test]
async fn listing_and_json_never_invoke_model() {
    let store = Arc::new(Store::open_memory().expect("store"));
    insert(
        &store,
        "milestone",
        "connection closed\nsecond line",
        0,
        "codex",
    );
    for json in [false, true] {
        let output = run_with_answer(store.clone(), None, "7d", 50, json, None, |_, _| async {
            panic!("listing must not invoke model")
        })
        .await
        .expect("listing")
        .0;
        if json {
            let records: serde_json::Value = serde_json::from_str(&output).expect("JSON");
            assert_eq!(
                records[0]["provider_excerpt"],
                "connection closed\nsecond line"
            );
        } else {
            assert!(output.contains("t-failure codex x1 connection closed"));
            assert!(!output.contains("second line"));
        }
    }
}

#[test]
fn failures_cli_review_defaults_and_limit_help() {
    use crate::cli::{
        Cli, Commands,
        agent_provider_args::{AgentArgs, AgentCommands},
    };
    use clap::Parser;
    for reviewer in [None, Some("codex")] {
        let mut args = vec!["aid", "agent", "failures", "--review"];
        if let Some(name) = reviewer {
            args.push(name);
        }
        match Cli::try_parse_from(args).expect("parse").command {
            Some(Commands::Agent(AgentArgs {
                action:
                    AgentCommands::Failures {
                        agent,
                        since,
                        limit,
                        json,
                        review,
                    },
            })) => {
                assert!(agent.is_none());
                assert_eq!(since, "7d");
                assert_eq!(limit, 50);
                assert!(!json);
                assert_eq!(review.as_deref(), Some(reviewer.unwrap_or("")));
            }
            _ => panic!("expected failures"),
        }
    }
    assert!(Cli::try_parse_from(["aid", "agent", "failures", "--json", "--review"]).is_err());
    let help = Cli::try_parse_from(["aid", "agent", "failures", "--help"])
        .err()
        .expect("help")
        .to_string();
    assert!(help.contains("before grouping"));
}

#[test]
fn redaction_does_not_merge_byte_distinct_excerpts() {
    let store = Store::open_memory().expect("store");
    insert(&store, "milestone", "key=synthetic-one", 0, "codex");
    insert(&store, "milestone", "key=synthetic-two", 1, "codex");
    let cutoff = Local::now() - duration("7d").expect("duration");
    let groups = load(&store, None, cutoff, 50).expect("groups");
    assert_eq!(groups.len(), 2);
    assert!(groups[0].time > groups[1].time);
    assert!(groups.iter().all(|group| group.count == 1));
    assert_eq!(groups[0].provider_excerpt, groups[1].provider_excerpt);
}

#[path = "agent_failure_redaction_tests.rs"]
mod redaction_cases;

#[path = "agent_failure_state_tests.rs"]
mod state_invariants;
