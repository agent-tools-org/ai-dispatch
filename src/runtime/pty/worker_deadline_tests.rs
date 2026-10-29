// Detached worker deadline regressions without external reaper or aid polling.
// Covers PTY and piped execution with active tools, output and default policy.
// Deps: real worker entry points, fake agent, isolated task store and shell children.

use super::*;
use std::time::{Duration, Instant};

async fn assert_deadline(pty: bool, streaming: bool, script: &str, default_max: bool) {
    let _permit = crate::support::test_subprocess::acquire();
    let temp = crate::support::test_env::TestHome::new();
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-detached-deadline".into());
    store.insert_task(&super::tests::task_fixture(&id, TaskStatus::Running)).expect("task");
    prepare_remote_wait(temp.path());
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", script]).current_dir(temp.path());
    let mut policy = crate::config::timeout_policy::TimeoutPolicy::default();
    if default_max {
        assert_eq!(policy.max_duration, Duration::from_secs(3600));
        assert_eq!(policy.hard_cap, Duration::from_secs(86400));
        policy.hard_cap = Duration::from_millis(400);
    } else {
        policy.max_duration = Duration::from_millis(400);
    }
    let log = crate::support::paths::log_path(id.as_str());
    let started = Instant::now();
    let agent = crate::runtime::watcher::idle_tests::FakeAgent;
    if pty {
        run_agent_process_with_control(
            false, &agent, &cmd, &id, &store, &log, None, None, streaming, policy, None, None,
            std::time::Instant::now(),
        )
        .expect("PTY worker");
    } else {
        crate::cmd::run::run_agent_process_with_cost(
            false, &agent,
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
            std::time::Instant::now(),
        )
        .await
        .expect("piped worker");
    }
    assert_result(&store, &id, started, pty);
}

fn prepare_remote_wait(dir: &Path) {
    let remote_wait = dir.join("ssh");
    crate::support::test_env::write_executable(&remote_wait, "#!/bin/sh\nsleep 6\n");
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
            .record_agent_completion(
                TaskCompletionUpdate {
                    exit_code: Some(0),
                    ..TaskCompletionUpdate::new(id.as_str(), TaskStatus::Done, 9_999)
                },
                None
            )
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
