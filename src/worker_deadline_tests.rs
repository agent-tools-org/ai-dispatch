// Detached worker deadline regressions without external reaper or aid polling.
// Covers PTY and piped execution with active tools, output and default policy.
// Deps: real worker entry points, fake agent, isolated task store and shell children.

use super::*;
use std::time::{Duration, Instant};

async fn assert_deadline(pty: bool, streaming: bool, script: &str, default_max: bool) {
    let _permit = crate::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-detached-deadline".into());
    store.insert_task(&super::tests::task_fixture(&id, TaskStatus::Running)).expect("task");
    prepare_remote_wait(temp.path());
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", script]).current_dir(temp.path());
    let mut policy = crate::timeout_policy::TimeoutPolicy::default();
    if default_max {
        assert_eq!(policy.max_duration, Duration::from_secs(3600));
        assert_eq!(policy.hard_cap, Duration::from_secs(86400));
        policy.hard_cap = Duration::from_millis(400);
    } else {
        policy.max_duration = Duration::from_millis(400);
    }
    let log = crate::paths::log_path(id.as_str());
    let started = Instant::now();
    let agent = crate::watcher::idle_tests::FakeAgent;
    if pty {
        run_agent_process_with_control(
            &agent, &cmd, &id, &store, &log, None, None, streaming, policy, None, None,
        )
        .expect("PTY worker");
    } else {
        crate::cmd::run::run_agent_process_with_cost(
            &agent,
            cmd.into(),
            &id,
            &store,
            &log,
            None,
            None,
            streaming,
            None,
            policy,
            None,
        )
        .await
        .expect("piped worker");
    }
    assert_result(&store, &id, started, pty);
}

fn prepare_remote_wait(dir: &Path) {
    let remote_wait = dir.join("ssh");
    std::fs::write(&remote_wait, "#!/bin/sh\nsleep 6\n").expect("remote wait");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&remote_wait, std::fs::Permissions::from_mode(0o700)).expect("mode");
    }
}

fn assert_result(store: &Store, id: &TaskId, started: Instant, pty: bool) {
    let task = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(task.status, TaskStatus::Failed);
    if !pty {
        let duration = task.duration_ms.expect("deadline duration");
        assert!(
            (350..2_000).contains(&duration),
            "duration near the 400ms cap: {duration}ms"
        );
    }
    assert!(
        !store
            .update_task_completion(TaskCompletionUpdate {
                id: id.as_str(),
                status: TaskStatus::Done,
                tokens: None,
                duration_ms: 9_999,
                observed_model: None,
                attribution_source: None,
                cost_usd: None,
                exit_code: Some(0),
            })
            .expect("late completion")
    );
    let later = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(later.status, task.status);
    assert_eq!(later.duration_ms, task.duration_ms);
    assert_eq!(later.completed_at, task.completed_at);
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "worker must stop before tool exits"
    );
    assert!(
        store
            .get_events(id.as_str())
            .expect("events")
            .iter()
            .any(|event| event.detail.contains("hard timeout")),
        "absolute timeout reason"
    );
}

const REMOTE_WAIT: &str = "printf 'ready\nready\n'; ./ssh & wait";
const CPU_BUSY: &str = "printf 'ready\nready\n'; yes >/dev/null & tool=$!; sleep 6; kill $tool; wait";
const OUTPUT_BUSY: &str = "i=0; while [ $i -lt 120 ]; do echo ready; i=$((i+1)); sleep .05; done";

#[tokio::test]
async fn detached_pty_stuck_remote_wait_hits_absolute_cap() {
    assert_deadline(true, true, REMOTE_WAIT, false).await;
}

#[tokio::test]
async fn detached_piped_stuck_remote_wait_hits_absolute_cap() {
    assert_deadline(false, true, REMOTE_WAIT, false).await;
}

#[tokio::test]
async fn detached_pty_cpu_busy_descendant_hits_absolute_cap() {
    assert_deadline(true, true, CPU_BUSY, false).await;
}

#[tokio::test]
async fn detached_piped_cpu_busy_descendant_hits_absolute_cap() {
    assert_deadline(false, true, CPU_BUSY, false).await;
}

#[tokio::test]
async fn detached_pty_explicit_timeout_bounds_continuous_output() {
    assert_deadline(true, true, OUTPUT_BUSY, false).await;
}

#[tokio::test]
async fn detached_piped_explicit_timeout_bounds_continuous_output() {
    assert_deadline(false, true, OUTPUT_BUSY, false).await;
}

#[tokio::test]
async fn detached_pty_default_policy_is_bounded_by_hard_cap() {
    assert_deadline(true, false, "sleep 6", true).await;
}

#[tokio::test]
async fn detached_piped_default_policy_is_bounded_by_hard_cap() {
    assert_deadline(false, false, "sleep 6", true).await;
}
