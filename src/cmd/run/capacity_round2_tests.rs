// Round-two regressions for retry registration, stdout provenance and refreshed holds.
// Exercises the backoff worker, process capture and shared dispatch resolver.

use super::*;
use std::process::Stdio;

#[tokio::test]
async fn provider_capacity_round2_retry_tree_stop_sees_waiting_child_before_backoff() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    install_agent(home.path(), "capacity-round2", "echo should-not-launch");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut row = task("t-round2-stop", home.path());
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("capacity-round2".into());
    store.insert_task(&row).expect("parent");
    let refusal = serde_json::json!({"type": "error", "message": CAPACITY});
    std::fs::write(paths::log_path(row.id.as_str()), refusal.to_string()).expect("log");
    let args = RunArgs {
        agent_name: "capacity-round2".into(),
        dir: row.effective_dir.clone(),
        ..Default::default()
    };
    let stopper = async {
        while store.get_events(row.id.as_str()).expect("events").is_empty() {
            tokio::task::yield_now().await;
        }
        let tree = store.get_retry_tree(row.id.as_str()).expect("tree");
        assert_eq!(tree.len(), 2, "retry child must exist before the wait");
        let child = tree.iter().find(|task| task.parent_task_id.is_some()).expect("child");
        assert_eq!(child.status, TaskStatus::Waiting);
        crate::cmd::stop::stop_retry_tree(&store, row.id.as_str(), false).expect("stop");
        assert_eq!(
            store.get_task(child.id.as_str()).expect("lookup").expect("child").status,
            TaskStatus::Stopped
        );
    };
    let (outcome, ()) = tokio::join!(continue_capacity(&store, &args, &row.id), stopper);
    assert!(outcome.expect("worker").is_none());
    assert_eq!(store.get_retry_tree(row.id.as_str()).expect("tree").len(), 2);
}

#[tokio::test]
async fn provider_capacity_round2_replayed_stderr_never_creates_hold_or_retry() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut row = task("t-round2-stderr", home.path());
    row.status = TaskStatus::Running;
    store.insert_task(&row).expect("task");
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", &format!("printf '%s\\n' '{}' >&2; exit 1",
            serde_json::json!({"type": "error", "message": CAPACITY}))])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().expect("child");
    let info = crate::watcher::watch_streaming(
        &crate::agent::codex::CodexAgent, &mut child, &row.id, &store,
        &paths::log_path(row.id.as_str()), None,
        crate::idle_timeout::DEFAULT_IDLE_TIMEOUT, None,
    ).await.expect("watch");
    assert_eq!(info.status, TaskStatus::Failed);
    assert!(std::fs::read_to_string(paths::log_path(row.id.as_str())).expect("log")
        .contains(CAPACITY), "stderr must still be replayed");
    crate::task_lifecycle::mark_failed(&store, &row.id).expect("failed");
    assert!(rate_limit::dispatch_blocking_hold_for_model(
        &row.agent, None, row.requested_model.as_deref()).is_none(), "stderr created hold");
    assert!(continue_capacity(&store, &RunArgs::default(), &row.id)
        .await.expect("capacity").is_none());
    assert_eq!(store.get_retry_tree(row.id.as_str()).expect("tree").len(), 1);
}

#[tokio::test]
async fn provider_capacity_round2_refreshed_hold_never_substitutes_or_clears_session() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _cache = crate::live_quota::CacheDirGuard::set(home.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid]);
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex"))
        .expect("peer model");
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-round2-refresh", home.path());
    store.insert_task(&row).expect("parent");
    let args = RunArgs {
        agent_name: "codex".into(), dir: row.effective_dir.clone(), ..Default::default()
    };
    let refresh = async {
        while store.get_events(row.id.as_str()).expect("events").is_empty() {
            tokio::task::yield_now().await;
        }
        rate_limit::mark_rate_limited_for_model(
            &row.agent, None, row.requested_model.as_deref(), CAPACITY);
    };
    let (child, ()) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(
            prepare_session(&store, &args, &row, row.requested_model.as_deref(), "expired hold"),
            refresh
        )
    }).await.expect("bounded backoff");
    let mut child = child.expect("backoff");
    let result = crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None);
    assert!(result.is_err(), "refreshed capacity must fail with its hold");
    assert_eq!(child.agent_name, "codex");
    assert_eq!(child.session_id, row.agent_session_id);
    assert!(result.err().expect("held").to_string().contains("provider capacity held until"));
}
