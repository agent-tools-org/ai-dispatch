// Lost-response, concurrent duplicate and conflicting-ID acceptance tests.
// Drives only the built aid CLI with a counted shell custom agent.
// Deps: isolated harness and bounded subprocess output capture.
#![cfg(unix)]
#[path = "common/caller_owned.rs"]
mod caller_owned;
mod common;
use caller_owned::{Harness, MODEL, PROTOCOL, SUCCESS, finish, identity, json, spawn};

#[test]
fn identical_resubmit_after_lost_response_returns_existing_without_launching() {
    let h = Harness::new(SUCCESS);
    let id = "t-lost-response";
    // Discard the successful submission body, as a disconnected caller would.
    let output = caller_owned::bounded(&mut h.run(id, &[]));
    assert!(output.status.success());
    h.settled(id);
    let existing = json(h.run(id, &[]), 0);
    assert_eq!(existing["submission"], "existing");
    identity(&existing, id);
    h.assert_rows(id, 1);
}

#[test]
fn racing_identical_submits_launch_exactly_one_agent() {
    let h = Harness::new(SUCCESS);
    let id = "t-concurrent";
    let first = spawn(&mut h.run(id, &[]));
    let second = spawn(&mut h.run(id, &[]));
    let mut submissions = Vec::new();
    for (child, stdout, stderr) in [first, second] {
        let output = finish(child, stdout, stderr);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        identity(&value, id);
        submissions.push(value["submission"].as_str().unwrap().to_string());
    }
    submissions.sort();
    assert_eq!(submissions, ["existing", "launched"]);
    h.settled(id);
    h.assert_rows(id, 1);
}

#[test]
fn conflicting_model_refuses_and_keeps_the_existing_row() {
    let h = Harness::new(SUCCESS);
    let id = "t-conflict-model";
    h.launch(id, &[]);
    let before = h.settled(id);
    let mut cmd = h.aid();
    cmd.args([
        "run",
        "controlled",
        "conflict",
        "--policy",
        "caller-owned",
        "--id",
        id,
        "--model",
        "different-model",
        "--json",
    ]);
    let conflict = json(cmd, 4);
    assert_eq!(conflict["protocol"], PROTOCOL);
    assert_eq!(conflict["error"]["code"], "id_conflict");
    let after = h.observe(id);
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(after["task"], before["task"]);
    assert_eq!(after["policy"], before["policy"]);
    h.assert_rows(id, 1);
}

#[test]
fn existing_default_mode_id_conflicts_without_suffix_or_row_change() {
    let h = Harness::new(SUCCESS);
    let id = "t-conflict-default";
    let mut cmd = h.aid();
    cmd.args([
        "run",
        "controlled",
        "default execution",
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
    assert!(caller_owned::bounded(&mut cmd).status.success());
    let before = json(h.aid().args(["show", id, "--json"]), 0);
    assert!(before["execution"]["policy"].is_null());
    let conflict = json(h.run(id, &[]), 4);
    assert_eq!(conflict["protocol"], PROTOCOL);
    assert_eq!(conflict["error"]["code"], "id_conflict");
    let after = json(h.aid().args(["show", id, "--json"]), 0);
    assert_eq!(after["execution"]["task_id"], id);
    assert_eq!(after["execution"]["task"]["requested_model"], MODEL);
    assert_eq!(after["execution"]["policy"], serde_json::Value::Null);
    for key in ["revision", "task", "worker", "policy"] {
        assert_eq!(after["execution"][key], before["execution"][key]);
    }
    assert_eq!(h.count(), 1);
    let rows = json(h.aid().args(["board", "--json"]), 0);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["id"], id);
}
