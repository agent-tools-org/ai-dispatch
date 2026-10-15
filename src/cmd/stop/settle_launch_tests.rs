// Agent launch regressions after a stopped task loses its process-start race.
// Covers PTY and buffered worker agent launch paths.
// Deps: real shell command, isolated AID home, stop command and process runners.

use super::{stop, tests::make_task};
use crate::{
    agent, support::paths,
    store::Store,
    types::{AgentKind, TaskId, TaskStatus},
};
use std::sync::Arc;

fn setup() -> (tempfile::TempDir, paths::AidHomeGuard, Arc<Store>, TaskId) {
    let home = tempfile::tempdir().expect("home");
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-stop-launch".into());
    store
        .insert_task(&make_task(id.as_str(), TaskStatus::Running))
        .expect("insert");
    stop(&store, id.as_str()).expect("stop");
    (home, guard, store, id)
}

fn command(marker: &std::path::Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "touch \"$1\"", "stop-test"]).arg(marker);
    cmd
}

fn assert_refused(store: &Store, id: &TaskId, marker: &std::path::Path, result: anyhow::Result<()>) {
    assert!(result.is_err(), "stopped task must refuse agent launch");
    assert!(!marker.exists(), "agent command must never execute");
    assert_eq!(
        store.get_task(id.as_str()).expect("get").expect("task").status,
        TaskStatus::Stopped
    );
    assert_eq!(store.get_events(id.as_str()).expect("events").len(), 1);
}

#[test]
fn settle_first_stopped_task_never_spawns_pty_agent() {
    let (home, _guard, store, id) = setup();
    let marker = home.path().join("launched");
    let result = crate::runtime::pty::runner::run_agent_process(
        agent::get_agent(AgentKind::Codex).as_ref(),
        &command(&marker),
        &id,
        &store,
        &paths::log_path(id.as_str()),
        None,
        None,
        true,
    );
    assert_refused(&store, &id, &marker, result);
}

#[tokio::test]
async fn settle_first_stopped_task_never_spawns_buffered_agent() {
    let (home, _guard, store, id) = setup();
    let marker = home.path().join("launched");
    let result = crate::cmd::run::run_agent_process_with_cost(
        agent::get_agent(AgentKind::Codex).as_ref(),
        tokio::process::Command::from(command(&marker)),
        &id,
        &store,
        &paths::log_path(id.as_str()),
        None,
        None,
        false,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
    )
    .await;
    assert_refused(&store, &id, &marker, result);
}
