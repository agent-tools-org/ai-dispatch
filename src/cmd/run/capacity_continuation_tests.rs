// Capacity continuation session, retry-budget and fake-agent lifecycle regressions.
// Deps: lifecycle, in-memory Store, isolated aid home and custom shell agent.

use super::*;
use crate::cmd::retry_logic::prepare_retry;
use crate::{support::paths, routing::rate_limit, routing::capacity as provider_capacity};

const CAPACITY: &str = "Selected model is at capacity. Please try a different model.";

fn task(id: &str, dir: &std::path::Path) -> Task {
    Task {
        prompt: "Original task".into(),
        agent_session_id: Some("capacity-session".into()),
        effective_dir: Some(dir.display().to_string()),
        requested_model: Some("gpt-6.1-sol".into()),
        ..crate::types::Task::test_fixture(TaskId(id.into()), TaskStatus::Failed)
    }
}

#[tokio::test]
async fn provider_capacity_ordinary_retry_reuses_codex_session_and_consumes_budget() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open_memory().expect("store");
    let mut row = task("t-capacity-args", dir.path());
    row.repo_path = row.effective_dir.clone();
    store.insert_task(&row).expect("task");
    let args = RunArgs {
        retry: 3,
        dir: Some(dir.path().display().to_string()),
        ..Default::default()
    };
    let child = prepare_retry(Arc::new(store), &row.id, &args).await
        .expect("ordinary retry").expect("retry args");
    assert_eq!(child.session_id.as_deref(), Some("capacity-session"));
    assert_eq!(child.retry, 2);
    assert_eq!(child.model, row.requested_model);
    assert_eq!(child.parent_task_id.as_deref(), Some(row.id.as_str()));
    assert!(child.prompt.starts_with("[Previous attempt failed]"));
    assert!(child.existing_task_id.is_none());
}

fn install_agent(home: &std::path::Path, name: &str, command: &str) {
    std::fs::create_dir_all(home.join("agents")).expect("agents");
    let args = serde_json::to_string(&vec!["-c", command]).expect("args");
    std::fs::write(
        home.join(format!("agents/{name}.toml")),
        format!(
            "[agent]\nid = '{name}'\ndisplay_name = '{name}'\ncommand = '/bin/sh'\n\
         prompt_mode = 'arg'\nstreaming = true\nfixed_args = {args}\ninteractive_input = false\n"
        ),
    )
    .expect("agent");
}

#[tokio::test]
async fn provider_capacity_refusal_keeps_hold_without_automatic_recovery() {
    let home = crate::support::test_env::TestHome::new();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![]);
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-capacity-no-route", home.path());
    store.insert_task(&row).expect("task");
    let error = serde_json::json!({"type":"error", "message":CAPACITY});
    seed_stdout(&store, &row.id, &error.to_string());
    rate_limit::mark_rate_limited_for_model(
        &row.agent,
        None,
        row.requested_model.as_deref(),
        CAPACITY,
    );
    let args = RunArgs {
        agent_name: "codex".into(),
        dir: row.effective_dir.clone(),
        ..Default::default()
    };
    assert!(
        continue_capacity(&store, &args, &row.id)
            .await
            .expect("capacity")
            .is_none()
    );
    let detail = rate_limit::dispatch_blocking_hold_for_model(
        &row.agent, None, row.requested_model.as_deref()).expect("hold");
    assert!(detail.contains("provider capacity held until"));
    let marker = std::fs::read_to_string(rate_limit::group_marker_path(
        &row.agent, None, &provider_capacity::group(row.requested_model.as_deref())))
        .expect("capacity marker");
    assert!(marker.contains(CAPACITY));
    assert_eq!(store.get_retry_tree(row.id.as_str()).expect("tree").len(), 1);
}

#[path = "capacity_continuation_flow_tests.rs"]
mod flow;

#[path = "capacity_cancellation_tests.rs"]
mod cancellation;

#[path = "capacity_round2_tests.rs"]
mod round2;

#[test]
fn provider_capacity_regression_never_selects_automatic_advice_fallback() {
    let home = crate::support::test_env::TestHome::new();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Grok]);
    let store = Store::open_memory().expect("store");
    let row = task("t-capacity-advice", home.path());
    store.insert_task(&row).expect("task");
    let args = RunArgs {
        agent_name: "codex".into(),
        ..Default::default()
    };
    rate_limit::mark_rate_limited_for_model(&row.agent, None, row.requested_model.as_deref(), CAPACITY);
    let mut child = RunArgs::for_retry(&store, &row).expect("ordinary retry args");
    child.dir = row.effective_dir.clone();
    assert_eq!(child.agent_name, args.agent_name);
    assert!(crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&Arc::new(store), &mut child, None)
        .is_err());
    assert_eq!(child.agent_name, "codex");
    assert_eq!(child.session_id, row.agent_session_id);
}

fn seed_stdout(store: &Store, id: &TaskId, raw: &str) {
    store.insert_event(&TaskEvent::new(id.clone(), EventKind::Error, CAPACITY.into()).with_metadata(Some(serde_json::json!({"provider_stdout": raw})))).expect("stdout event");
}

async fn continue_capacity(store: &Arc<Store>, args: &RunArgs, id: &TaskId) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(id.as_str())? else { return Ok(None) };
    if task.status != TaskStatus::Failed || provider_capacity::read(store, id, task.agent).is_none() {
        return Ok(None);
    }
    crate::cmd::run::run_lifecycle::retry_or_cascade(store, args, id).await
}

#[path = "capacity_scope_cut_tests.rs"]
mod scope_cut;
