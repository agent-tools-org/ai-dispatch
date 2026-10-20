// Worker deadline regression using a real piped Claude result stream.
// Ensures cancelling the watcher settles usage from its durable checkpoint.
use super::*;
use crate::agent::claude::ClaudeAgent;
use std::time::Duration;

#[tokio::test]
async fn worker_timeout_recovers_ingested_claude_usage() {
    let _permit = crate::support::test_subprocess::acquire();
    let dir = tempfile::tempdir().expect("dir");
    let _home = crate::support::paths::AidHomeGuard::set(dir.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-usage-timeout".into());
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES (?1, 'claude', '', 'running', '2026-10-20T00:00:00Z')",
            [id.as_str()],
        )
        .expect("task");
    let input = dir.path().join("results.jsonl");
    std::fs::write(
        &input,
        include_str!(
            "../../../tests/fixtures/streaming_completion/claude-cumulative-results.jsonl"
        ),
    )
    .expect("fixture");
    let log = dir.path().join("agent.log");
    let mut args = timeout_args(&store, &id, &log);
    args.cmd.args(["-c", "cat \"$1\"; exec sleep 10", "replay"]);
    args.cmd.arg(input);
    let mut child = spawn_agent(&mut args).expect("spawn");
    let result = watch_agent(&args, &mut child, Instant::now())
        .await
        .expect("watch");
    let _ = child.kill().await;
    let _ = child.wait().await;
    assert_eq!(result.status, TaskStatus::Failed);
    let task = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.tokens, Some(10_633_915));
    assert!((task.cost_usd.expect("cost") - 4.5095448).abs() < 1e-12);
}

fn timeout_args<'a>(
    store: &'a Arc<Store>,
    id: &'a TaskId,
    log: &'a std::path::Path,
) -> RunProcessArgs<'a> {
    let cmd = Command::new("sh");
    RunProcessArgs {
        agent: &ClaudeAgent,
        cmd,
        task_id: id,
        store,
        log_path: log,
        output_path: None,
        model: None,
        streaming: true,
        workgroup_id: None,
        timeout_policy: crate::config::timeout_policy::TimeoutPolicy {
            max_duration: Duration::from_secs(1),
            idle: Duration::from_secs(10),
            ..Default::default()
        },
        max_task_cost: None,
    }
}

#[tokio::test]
async fn worker_requests_wrap_up_without_reaper_at_effective_deadline() {
    let _permit = crate::support::test_subprocess::acquire();
    let dir = tempfile::tempdir().expect("dir");
    let _home = crate::support::paths::AidHomeGuard::set(dir.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-worker-wrap".into());
    store.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at, read_only)
         VALUES (?1, 'gemini', '', 'running', ?2, 1)",
        rusqlite::params![id.as_str(), Local::now().to_rfc3339()],
    ).expect("task");
    let log = dir.path().join("agent.log");
    let mut args = timeout_args(&store, &id, &log);
    args.streaming = false;
    args.timeout_policy.max_duration = Duration::from_secs(1200);
    args.timeout_policy.hard_cap = Duration::from_secs(600);
    args.cmd.args(["-c", "sleep 0.2"]);
    let mut child = spawn_agent(&mut args).expect("spawn");
    let start = Instant::now() - Duration::from_millis(539_990);
    watch_agent(&args, &mut child, start).await.expect("watch");
    let messages = store.list_messages_for_task(id.as_str()).expect("messages");
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.contains("stop investigating"));
    assert!(messages[0].content.contains("evidence"));
    assert!(messages[0].content.contains("open questions"));
    assert!(!messages[0].content.contains("commit"));
    assert!(store.get_task(id.as_str()).expect("query").expect("task").read_only);
    assert!(!crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 541, 600)
        .expect("backup poll"));
    assert_eq!(store.list_messages_for_task(id.as_str()).expect("messages").len(), 1);
}
