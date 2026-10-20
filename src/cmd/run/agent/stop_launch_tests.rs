// Foreground timeout launch regression after user stop wins.
// Deps: real shell command, isolated AID home and the timeout process runner.

use super::{task, TimeoutTestAgent};
use crate::{
    support::paths,
    store::Store,
    types::{TaskId, TaskStatus},
};
use std::sync::Arc;

#[tokio::test]
async fn settle_first_stopped_task_never_spawns_timeout_agent() {
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-stop-timeout-launch".into());
    store
        .insert_task(&task(&id, TaskStatus::Running))
        .expect("task");
    crate::cmd::stop::stop(&store, id.as_str()).expect("stop");
    let marker = home.path().join("launched");
    let mut cmd = tokio::process::Command::new("sh");
    cmd.args(["-c", "touch \"$1\"", "stop-test"]).arg(&marker);
    let result = super::run_agent_process_with_timeout(
        &TimeoutTestAgent,
        cmd,
        &id,
        &store,
        &paths::log_path(id.as_str()),
        None,
        None,
        false,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
        std::time::Instant::now(),
    )
    .await;
    assert!(result.is_err(), "stopped task must refuse agent launch");
    assert!(!marker.exists(), "agent command must never execute");
    assert_eq!(
        store
            .get_task(id.as_str())
            .expect("get")
            .expect("task")
            .status,
        TaskStatus::Stopped
    );
    assert_eq!(store.get_events(id.as_str()).expect("events").len(), 1);
}
