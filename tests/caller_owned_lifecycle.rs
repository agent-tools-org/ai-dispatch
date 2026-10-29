// Caller-owned settlement, stop, lost-worker and persisted revision acceptance tests.
// A gated verifier holds the worker after terminal status without store manipulation.
// Deps: public CLI receipts, counted shell agent, libc for the requested SIGKILL.
#![cfg(unix)]
#[path = "common/caller_owned.rs"]
mod caller_owned;
mod common;
#[path = "common/caller_owned_lifecycle.rs"]
mod lifecycle;
use caller_owned::{Harness, MODEL, SUCCESS, bounded, identity, json};
use lifecycle::{AgentProcess, GROUP_AGENT_SCRIPT, observe_unsettled};
use std::time::{Duration, Instant};

#[test]
fn terminal_status_does_not_satisfy_settlement_wait() {
    let h = Harness::new(SUCCESS);
    let id = "t-terminal-window";
    let early = h.terminal_window(id);
    h.assert_rows(id, 1);
    h.release();
    let late = h.settled(id);
    assert!(late["revision"].as_u64().unwrap() > early["revision"].as_u64().unwrap());
    h.assert_rows(id, 1);
}

#[test]
fn stop_running_then_stop_settled_and_observe_the_same_id() {
    let h = Harness::new("exec sleep 8");
    let id = "t-stop-running";
    h.launch(id, &[]);
    h.wait_invoked();
    let stopped = h.stop(id);
    assert_eq!(stopped["stop"]["result"], "stopped");
    assert_eq!(stopped["task"]["terminal"], true);
    assert_eq!(stopped["worker"]["outcome"], "stopped");
    assert_eq!(h.settled(id)["worker"]["outcome"], "stopped");
    let again = h.stop(id);
    assert_eq!(again["stop"]["result"], "already_settled");
    assert_eq!(again["revision"], stopped["revision"]);
    h.assert_rows(id, 1);
}

#[test]
fn stop_signals_a_terminal_but_unsettled_worker() {
    let h = Harness::new(SUCCESS);
    let id = "t-stop-terminal";
    let early = h.terminal_window(id);
    let pid = early["worker"]["pid"].as_i64().expect("live worker PID") as i32;
    let stopped = h.stop(id);
    assert_eq!(stopped["stop"]["result"], "stopped");
    assert_eq!(stopped["task"]["terminal"], true);
    assert_eq!(stopped["worker"]["outcome"], "stopped");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "stop left the worker alive");
    assert_eq!(h.settled(id)["worker"]["outcome"], "stopped");
    h.assert_rows(id, 1);
}

#[test]
fn killed_worker_is_lost_and_duplicate_submission_never_relaunches() {
    let h = Harness::new(GROUP_AGENT_SCRIPT);
    let id = "t-lost-worker";
    h.launch(id, &[]);
    h.wait_for(|| h.path("aid/agent.pid").exists());
    let mut agent = AgentProcess::from_marker(&h, "agent");
    let mut child = AgentProcess::from_marker(&h, "agent-child");
    let group = unsafe { libc::getpgid(agent.pid) };
    assert!(group > 0);
    assert_eq!(unsafe { libc::getpgid(child.pid) }, group);
    let mut running = h.observe(id);
    h.wait_for(|| {
        running = h.observe(id);
        running["worker"]["state"] == "running" && running["worker"]["pid"].is_number()
    });
    let pid = running["worker"]["pid"].as_i64().unwrap() as i32;
    assert_ne!(pid, agent.pid);
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let waiting = observe_unsettled(&h, &agent, id);
    assert!(waiting["revision"].as_u64().unwrap() >= running["revision"].as_u64().unwrap());
    agent.kill();
    h.wait_for(|| !agent.is_alive());
    assert_eq!(unsafe { libc::getpgid(child.pid) }, group);
    let child_waiting = observe_unsettled(&h, &child, id);
    assert!(child_waiting["revision"].as_u64().unwrap() >= waiting["revision"].as_u64().unwrap());
    assert_eq!(h.count(), 1);
    child.kill();
    let lost = json(h.aid().args(["wait", id, "--json", "--timeout", "25"]), 0);
    identity(&lost, id);
    assert_eq!(lost["settled"], true);
    assert_eq!(lost["worker"]["state"], "settled");
    assert_eq!(lost["worker"]["outcome"], "lost");
    assert!(lost["revision"].as_u64().unwrap() > child_waiting["revision"].as_u64().unwrap());
    let existing = json(h.run(id, &[]), 0);
    assert_eq!(existing["submission"], "existing");
    identity(&existing, id);
    assert_eq!(existing["worker"]["outcome"], "lost");
    assert_eq!(existing["settled"], true);
    assert_eq!(existing["revision"], lost["revision"]);
    h.assert_rows(id, 1);
}

#[test]
fn unconfirmed_worker_waits_for_the_full_timeout() {
    let h = Harness::new(SUCCESS);
    let id = "t-unconfirmed-wait";
    // A default-mode dry run leaves a skipped row without a worker spec or settlement.
    let mut cmd = h.aid();
    cmd.args([
        "run",
        "controlled",
        "preview only",
        "--id",
        id,
        "--model",
        MODEL,
        "--dry-run",
        "--no-hint",
        "--no-skill",
    ]);
    assert!(bounded(&mut cmd).status.success());
    let early = json(h.aid().args(["wait", id, "--json", "--timeout", "0"]), 124);
    assert_eq!(early["worker"]["state"], "unconfirmed");
    assert_eq!(early["settled"], false);
    let start = Instant::now();
    let later = json(h.aid().args(["wait", id, "--json", "--timeout", "2"]), 124);
    assert!(
        start.elapsed() >= Duration::from_secs(2),
        "unconfirmed wait returned early"
    );
    assert_eq!(later["task_id"], id);
    assert_eq!(later["task"]["requested_model"], MODEL);
    assert_eq!(later["worker"]["state"], "unconfirmed");
    assert_eq!(later["settled"], false);
    assert!(later["policy"].is_null());
    let stopped = json(h.aid().args(["stop", id, "--json"]), 0);
    assert_eq!(stopped["settled"], true);
    let observed = json(h.aid().args(["wait", id, "--json", "--timeout", "0"]), 0);
    assert_eq!(observed["task_id"], id);
    assert_eq!(observed["revision"], stopped["revision"]);
    assert_default_rows(&h, id, 0);
}

#[test]
fn settled_default_done_task_keeps_revision_and_text_stop_refuses() {
    let h = Harness::new(SUCCESS);
    let id = "t-default-done-stop";
    let mut cmd = h.aid();
    cmd.args([
        "run",
        "controlled",
        "finish once",
        "--id",
        id,
        "--model",
        MODEL,
        "--no-hint",
        "--no-skill",
        "--timeout",
        "15",
    ])
    .arg("--dir")
    .arg(h.path("project"));
    assert!(bounded(&mut cmd).status.success());
    let done = json(h.aid().args(["wait", id, "--json", "--timeout", "25"]), 0);
    assert_eq!(done["task"]["status"], "done");
    assert_eq!(done["settled"], true);
    let stopped = json(h.aid().args(["stop", id, "--json"]), 0);
    assert_eq!(stopped["stop"]["result"], "already_settled");
    assert_eq!(stopped["revision"], done["revision"]);
    let text = bounded(h.aid().args(["stop", id]));
    assert!(!text.status.success());
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&text.stdout),
        String::from_utf8_lossy(&text.stderr)
    );
    assert!(output.contains("task already done"), "{output}");
    let after = json(h.aid().args(["show", id, "--json"]), 0);
    assert_eq!(after["execution"]["revision"], done["revision"]);
    assert_default_rows(&h, id, 1);
}

fn assert_default_rows(h: &Harness, id: &str, invocations: usize) {
    assert_eq!(h.count(), invocations);
    let rows = json(h.aid().args(["board", "--json"]), 0);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["id"], id);
    assert_eq!(rows[0]["requested_model"], MODEL);
    let shown = json(h.aid().args(["show", id, "--json"]), 0);
    assert_eq!(shown["execution"]["task_id"], id);
    assert!(shown["execution"]["policy"].is_null());
}

#[test]
fn revisions_never_decrease_and_fresh_process_reads_persisted_receipt() {
    let h = Harness::new(SUCCESS);
    let id = "t-revisions";
    let early = h.terminal_window(id);
    let pid = early["worker"]["pid"].as_i64().unwrap() as i32;
    let mut previous = early;
    let mut revision = previous["revision"].as_u64().unwrap();
    for _ in 0..3 {
        let value = h.observe(id);
        let next = value["revision"].as_u64().unwrap();
        assert!(next >= revision);
        if next == revision {
            for key in ["task", "worker", "policy", "settled"] {
                assert_eq!(value[key], previous[key], "equal revision changed {key}");
            }
        }
        previous = value;
        revision = next;
    }
    h.release();
    let last = h.settled(id);
    assert!(last["revision"].as_u64().unwrap() > revision);
    h.wait_for(|| unsafe { libc::kill(pid, 0) } == -1);
    let fresh = h.observe(id);
    // observed_at is per observation, not persisted lifecycle state.
    let mut expected = last;
    let mut actual = fresh;
    expected.as_object_mut().unwrap().remove("observed_at");
    actual.as_object_mut().unwrap().remove("observed_at");
    assert_eq!(actual, expected);
    let shown = json(h.aid().args(["show", id, "--json"]), 0);
    let mut embedded = shown["execution"].clone();
    embedded.as_object_mut().unwrap().remove("observed_at");
    assert_eq!(embedded, expected);
    h.assert_rows(id, 1);
}
