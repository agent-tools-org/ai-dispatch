// Round-two regressions for retry registration, stdout provenance and refreshed holds.
// Exercises the backoff worker, process capture and shared dispatch resolver.

use super::*;
use std::process::Stdio;

#[tokio::test]
async fn provider_capacity_explicit_waiting_retry_tree_stop_rejects_replacement() {
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
    seed_stdout(&store, &row.id, &refusal.to_string());
    let args = RunArgs {
        agent_name: "capacity-round2".into(),
        dir: row.effective_dir.clone(),
        ..Default::default()
    };
    let mut waiting = row.clone();
    waiting.id = TaskId::generate();
    waiting.parent_task_id = Some(row.id.to_string());
    waiting.status = TaskStatus::Waiting;
    store.insert_task(&waiting).expect("ordinary waiting fixture");
    crate::cmd::stop::stop_retry_tree(&store, row.id.as_str(), false).expect("stop");
    assert!(continue_capacity(&store, &args, &row.id).await.expect("capacity").is_none());
    let child = waiting.id;
    assert_eq!(store.get_task(child.as_str()).expect("lookup").expect("child").status,
        TaskStatus::Stopped);
    let mut stopped_args = RunArgs::for_retry(&store, &store.get_task(child.as_str())
        .expect("lookup").expect("child")).expect("args");
    stopped_args.existing_task_id = Some(child);
    assert!(crate::cmd::run::run(store.clone(), stopped_args).await.is_err());
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
async fn provider_capacity_ordinary_retry_refreshed_hold_never_substitutes_or_clears_session() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _cache = crate::live_quota::CacheDirGuard::set(home.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid]);
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex"))
        .expect("peer model");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut row = task("t-round2-refresh", home.path());
    row.repo_path = row.effective_dir.clone();
    store.insert_task(&row).expect("parent");
    seed_stdout(&store, &row.id,
        &serde_json::json!({"type": "error", "message": CAPACITY}).to_string());
    let args = RunArgs {
        agent_name: "codex".into(), dir: row.effective_dir.clone(), ..Default::default()
    };
    let mut child = prepare_retry(store.clone(), &row.id, &RunArgs { retry: 1, ..args })
        .await.expect("ordinary backoff").expect("ordinary retry");
    assert_eq!(child.retry, 0);
    assert!(child.existing_task_id.is_none());
    rate_limit::mark_rate_limited_for_model(
        &row.agent, None, row.requested_model.as_deref(), CAPACITY);
    let result = crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None);
    assert!(result.is_err(), "refreshed capacity must fail with its hold");
    assert_eq!(child.agent_name, "codex");
    assert_eq!(child.session_id, row.agent_session_id);
    assert!(result.err().expect("held").to_string().contains("provider capacity held until"));
    child.cascade = vec!["droid".into()];
    assert!(crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None).is_err());
    assert_eq!(child.agent_name, "codex");
    assert_eq!(child.session_id, row.agent_session_id);
    assert_eq!(child.cascade, ["droid"]);
    assert!(crate::cmd::run::run(store.clone(), child).await.is_err());
    assert_eq!(store.get_retry_tree(row.id.as_str()).expect("tree").len(), 1);
}

#[test]
fn provider_capacity_round2_pins_resolved_model_when_served_cache_changes() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _served = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Codex, Some(vec!["other-model".into()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-round2-pin", home.path());
    store.insert_task(&row).expect("task");
    let args = RunArgs { model: row.requested_model.clone(),
        model_source: crate::agent::model_validation::ModelSource::UserSupplied,
        dir: row.effective_dir.clone(), ..Default::default() };
    store.update_task_dispatch_args(row.id.as_str(), &args.dispatch_args_json().expect("json"))
        .expect("saved args");
    let mut child = RunArgs::for_retry(&store, &row).expect("ordinary retry args");
    let error = crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None)
        .err().expect("unserved retained model must fail");
    assert!(error.to_string().contains("does not serve model"));
    assert_eq!(child.agent_name, "codex");
    assert_eq!(child.model, row.requested_model);
    assert_eq!(child.session_id, row.agent_session_id);
}

#[test]
fn provider_capacity_round2_classifies_decoded_turn_failure_and_reconnect_sequence() {
    let refusal = serde_json::json!({"type": "turn.failed", "error": {"message": CAPACITY}});
    let reconnect = serde_json::json!({"type": "error", "message":
        "Reconnecting... 5/5 (unexpected status 503 Service Unavailable)"});
    let failure = serde_json::json!({"type": "turn.failed", "error": {"message": "stream disconnected"}});
    let completed = serde_json::json!({"type": "turn.completed", "usage": {}});
    for (lines, capacity) in [
        (vec![refusal], true), (vec![reconnect.clone(), failure.clone()], true),
        (vec![reconnect, completed, failure], false),
    ] {
        let home = tempfile::tempdir().expect("home");
        let _home = paths::AidHomeGuard::set(home.path());
        let store = Arc::new(Store::open_memory().expect("store"));
        let row = task("t-round2-decoded", home.path());
        store.insert_task(&row).expect("task");
        let mut info = crate::agent::Agent::parse_completion(&crate::agent::codex::CodexAgent, "");
        let mut tracker = crate::watcher::SyntheticMilestoneTracker::new();
        let mut count = 0;
        let mut session_saved = false;
        for line in lines {
            crate::watcher::handle_streaming_line_with_session(crate::watcher::StreamLineContext {
                agent: &crate::agent::codex::CodexAgent, task_id: &row.id, store: &store,
                workgroup_id: None, synthetic_tracker: &mut tracker,
            }, &mut info, &mut count, &line.to_string(), &mut session_saved).expect("decode");
        }
        assert_eq!(provider_capacity::read(&store, &row.id, row.agent).is_some(), capacity);
    }
}

#[tokio::test]
async fn provider_capacity_round2_ordinary_cascade_preserves_prompt_and_completion_hook() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    install_agent(home.path(), "ordinary-cascade", "echo completed");
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-round2-ordinary", home.path());
    store.insert_task(&row).expect("parent");
    let marker = home.path().join("cascade-hook");
    let args = RunArgs {
        agent_name: "codex".into(), prompt: "Normal cascade feedback".into(),
        dir: row.effective_dir.clone(), cascade: vec!["ordinary-cascade".into()],
        on_done: Some(format!("printf done > '{}'", marker.display())), foreground: true, no_audit: true,
        skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.into()], ..Default::default()
    };
    let id = crate::cmd::run::run_lifecycle::retry_or_cascade(&store, &args, &row.id)
        .await.expect("cascade").expect("child");
    let saved = RunArgs::saved_for_task(&store, id.as_str()).expect("saved").expect("args");
    assert_eq!(saved.prompt, args.prompt);
    assert_eq!(saved.on_done, args.on_done);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::task::yield_now().await;
        }
    }).await.expect("ordinary cascade completion hook");
    assert_eq!(std::fs::read_to_string(marker).expect("hook output"), "done");
}
