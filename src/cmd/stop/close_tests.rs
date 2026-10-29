// Stop must finish cleanup before settlement and preserve default text terminal behavior.
// Competing closers determine the final JSON stop result, rather than the initial snapshot.
use super::*;
use crate::support::test_env::TestHome;

#[test]
fn caller_owned_stop_cleanup_failure_never_writes_settlement() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let repo = crate::support::test_repo::repo_with_file("base.txt", "base\n", "base");
    let mut task = Task::test_fixture(TaskId("t-stop-cleanup-fails".into()), TaskStatus::Running);
    task.worktree_path = Some(repo.path().display().to_string());
    store.insert_task(&task).expect("task");
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_final_state BEFORE UPDATE OF final_head_sha ON tasks
        BEGIN SELECT RAISE(ABORT, 'final state unavailable'); END;",
        )
        .expect("inject cleanup failure");
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("JSON stop"), 5);
    let worker = store
        .execution_facts(task.id.as_str())
        .expect("facts")
        .expect("row")
        .settlement
        .expect("worker");
    assert_eq!(worker.state, "unconfirmed");
    assert!(worker.outcome.is_none());
    assert!(worker.settled_at.is_none());
}

#[cfg(unix)]
#[test]
fn default_text_stop_terminal_live_worker_is_untouched_as_on_main() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-default-terminal-stop".into()), TaskStatus::Failed);
    store.insert_task(&task).expect("task");
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 60", "__run-task", task.id.as_str()]);
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    super::settle_first_tests::save_worker(task.id.as_str(), worker.id());
    let before = store.execution_snapshot(task.id.as_str()).expect("snapshot");
    let spec = std::fs::read(crate::support::paths::job_path(task.id.as_str())).expect("spec bytes");
    assert!(
        stop(&store, task.id.as_str())
            .expect_err("terminal error")
            .to_string()
            .starts_with("task already failed:")
    );
    assert!(worker.child_mut().try_wait().expect("owner wait").is_none());
    assert_eq!(
        store.execution_snapshot(task.id.as_str()).expect("snapshot"),
        before
    );
    assert_eq!(
        std::fs::read(crate::support::paths::job_path(task.id.as_str())).expect("spec bytes"),
        spec
    );
}

#[test]
fn caller_owned_stop_reports_settlement_written_during_its_cleanup_as_existing() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-stop-other-closer".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let spec = background::BackgroundRunSpec {
        task_id: task.id.to_string(),
        ..Default::default()
    };
    background::save_spec(&spec).expect("spec");
    assert_ne!(
        store
            .execution_snapshot(task.id.as_str())
            .expect("snapshot")
            .expect("row")["worker"]["state"],
        "settled"
    );
    let other_store = store.clone();
    background::BackgroundRunSpec::on_next_save(move || {
        assert_eq!(
            close_execution(
                &other_store,
                &spec.task_id,
                "completed",
                Closer::Worker,
                Some(&spec)
            )
            .expect("other closer"),
            Closed::Written
        );
    });
    let (_, closed) = terminate_task(
        &store,
        task.id.as_str(),
        true,
        "Task stopped by user",
        "stopped",
        true,
    )
    .expect("stop");
    assert_eq!(closed, Closed::Existing);
    assert_eq!(json::stop_result(&closed), "already_settled");
}

#[cfg(unix)]
#[test]
fn caller_owned_stop_kills_recorded_tools_in_their_own_sessions() {
    use std::io::{BufRead, BufReader};
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-stop-tool-session".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let mut command = std::process::Command::new("sh");
    command
        .args([
            "-c",
            "perl -e 'setpgrp(0, 0); exec q(sleep), q(60)' & echo $!; wait",
            "__run-task",
            task.id.as_str(),
        ])
        .stdout(std::process::Stdio::piped());
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    let mut output = BufReader::new(worker.child_mut().stdout.take().expect("stdout"));
    let mut line = String::new();
    output.read_line(&mut line).expect("tool pid");
    let tool = line.trim().parse::<u32>().expect("tool pid");
    super::settle_first_tests::save_worker(task.id.as_str(), worker.id());
    let result = stop_json(&store, task.id.as_str(), true).expect("stop");
    let alive = crate::support::test_subprocess::is_live(tool as i32);
    if alive {
        unsafe {
            libc::kill(tool as i32, libc::SIGKILL);
        }
    }
    assert_eq!(result, 0);
    assert!(!alive, "stop must kill the tool before it settles");
    assert!(
        worker.child_mut().wait().is_ok(),
        "only the owner reaps the worker"
    );
}
