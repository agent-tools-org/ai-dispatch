// Stop refuses unverified agent groups while still stopping a verified worker.
// Descendant snapshots precede signals and failed stops never claim a stopped outcome.
use super::*;
use super::close_tests::agent_fixture;
use crate::support::test_env::TestHome;

#[test]
fn caller_owned_identity_mismatch_does_not_signal_or_settle_a_live_group() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut agent) = agent_fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process=json_set(agent_process,'$.started','previous') WHERE id=?1",
            [task.id.as_str()],
        )
        .expect("mismatch");
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("stop"), 5);
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
    assert!(!store.execution_process(task.id.as_str()).expect("evidence").2);
    agent.force_kill();
    agent.child_mut().wait().expect("natural finish");
    background::clear_spec(task.id.as_str()).expect("completion removes spec");
    assert_eq!(
        crate::cmd::execution_receipt::receipt(&store, task.id.as_str()).expect("completion")["worker"]["outcome"],
        "completed"
    );
}

#[test]
fn caller_owned_missing_agent_evidence_does_not_prevent_signalling_the_verified_worker() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-worker-identity".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    store.test_caller_owned(task.id.as_str());
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process=NULL,worker_spawned=1 WHERE id=?1",
            [task.id.as_str()],
        )
        .expect("unknown agent");
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 60", "__run-task", task.id.as_str()]);
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    super::settle_first_tests::save_worker(task.id.as_str(), worker.id());
    assert_eq!(
        stop_json(&store, task.id.as_str(), true).expect("unconfirmed agent"),
        5
    );
    assert!(
        !worker
            .child_mut()
            .wait()
            .expect("worker was killed but never reaped by stop")
            .success()
    );
    assert!(!store.execution_process(task.id.as_str()).expect("evidence").2);
}

#[test]
fn caller_owned_waiting_placeholder_can_be_stopped_without_inventing_an_agent_identity() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    store
        .insert_waiting_task(
            "t-waiting-stop",
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .expect("waiting");
    assert_eq!(
        store
            .execution_process("t-waiting-stop")
            .expect("evidence")
            .0
            .expect("not started")
            .state,
        "not_started"
    );
    store.test_caller_owned("t-waiting-stop");
    assert_eq!(
        stop_json(&store, "t-waiting-stop", true).expect("stop waiting"),
        0
    );
    assert_eq!(
        crate::cmd::execution_receipt::receipt(&store, "t-waiting-stop").expect("receipt")["worker"]["outcome"],
        "stopped"
    );
}

#[test]
fn caller_owned_stop_signals_agent_descendants_in_separate_sessions_before_the_group_disappears() {
    use std::io::{BufRead, BufReader};
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-agent-tools".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    store.test_caller_owned(task.id.as_str());
    let mut agent = store
        .with_running_task(true, task.id.as_str(), |_pid| {
            let mut command = std::process::Command::new("sh");
            command
                .args([
                    "-c",
                    "perl -e 'setpgrp(0,0); exec q(sleep),q(60)' & echo $!; wait",
                ])
                .stdout(std::process::Stdio::piped());
            let agent = crate::runtime::process::guard::ProcessGuard::spawn(&mut command)?;
            *_pid = Some((agent.id(), crate::runtime::process::monitor::activity::root_started(agent.id())));
            background::update_agent_pid(task.id.as_str(), agent.id())?;
            Ok(agent)
        })
        .expect("agent");
    let mut line = String::new();
    BufReader::new(agent.child_mut().stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("tool pid");
    let tool: i32 = line.trim().parse().expect("pid");
    wait_for_own_group(tool);
    let code = stop_json(&store, task.id.as_str(), true).expect("stop");
    let dead = crate::support::test_subprocess::wait_dead(tool, std::time::Duration::from_secs(5));
    if !dead {
        unsafe {
            libc::kill(tool, libc::SIGKILL);
        }
    }
    assert_eq!(code, 0);
    assert!(dead, "separate-session tool must be signalled too");
    agent.child_mut().wait().expect("only owner reaps agent");
}

fn wait_for_own_group(pid: i32) {
    for _ in 0..100 {
        if unsafe { libc::getpgid(pid) } == pid {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("tool must enter its own group");
}

#[test]
fn caller_owned_stop_signals_worker_descendants_in_separate_sessions() {
    use std::io::{BufRead, BufReader};
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-worker-tools".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    store.test_caller_owned(task.id.as_str());
    let mut command = std::process::Command::new("sh");
    command
        .args([
            "-c",
            "perl -e 'setpgrp(0,0); exec q(sleep),q(60)' & echo $!; wait",
            "__run-task",
            task.id.as_str(),
        ])
        .stdout(std::process::Stdio::piped());
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    let mut line = String::new();
    BufReader::new(worker.child_mut().stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("tool");
    let tool: i32 = line.trim().parse().expect("tool pid");
    wait_for_own_group(tool);
    super::settle_first_tests::save_worker(task.id.as_str(), worker.id());
    let code = stop_json(&store, task.id.as_str(), true).expect("stop");
    let dead = crate::support::test_subprocess::wait_dead(tool, std::time::Duration::from_secs(5));
    if !dead {
        unsafe {
            libc::kill(tool, libc::SIGKILL);
        }
    }
    assert_eq!(code, 0);
    assert!(dead, "worker tool must be signalled too");
    worker.child_mut().wait().expect("owner wait");
}
