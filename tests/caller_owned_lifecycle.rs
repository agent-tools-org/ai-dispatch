// Caller-owned settlement, stop, lost-worker and persisted revision acceptance tests.
// A gated verifier holds the worker after terminal status without store manipulation.
// Deps: public CLI receipts, counted shell agent, libc for the requested SIGKILL.
#![cfg(unix)]
#[path = "common/caller_owned.rs"]
mod caller_owned;
mod common;
use caller_owned::{Harness, SUCCESS, json};

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
    let h = Harness::new("exec sleep 8");
    let id = "t-lost-worker";
    h.launch(id, &[]);
    h.wait_invoked();
    let mut running = h.observe(id);
    h.wait_for(|| {
        running = h.observe(id);
        running["worker"]["state"] == "running" && running["worker"]["pid"].is_number()
    });
    let pid = running["worker"]["pid"].as_i64().unwrap() as i32;
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let (code, observed) = caller_owned::result(h.aid().args(["wait", id, "--json", "--timeout", "12"]));
    caller_owned::identity(&observed, id);
    if observed["settled"] == true {
        assert_eq!(code, 0);
        assert_eq!(observed["worker"]["outcome"], "lost");
    } else {
        assert_eq!(code, 124);
        assert_eq!(observed["worker"]["state"], "unconfirmed");
    }
    assert!(observed["revision"].as_u64().unwrap() >= running["revision"].as_u64().unwrap());
    let existing = json(h.run(id, &[]), 0);
    assert_eq!(existing["submission"], "existing");
    caller_owned::identity(&existing, id);
    h.assert_rows(id, 1);
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
