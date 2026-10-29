// Caller-owned executions never multiply when automatic recovery triggers fire.
// Exercises model, hang, dirty delivery, quota, verify and checklist failures via CLI.
// Deps: counted shell agent harness and the built aid binary.
#![cfg(unix)]
#[path = "common/caller_owned.rs"]
mod caller_owned;
mod common;
#[path = "common/caller_owned_recovery.rs"]
mod recovery;
use caller_owned::SUCCESS;
use recovery::{Recovery, first_attempt};

fn failed_once(body: &str, extra: &[&str]) {
    let fixture = Recovery::new(body);
    let id = "t-recovery";
    fixture.start(id, true, extra);
    let receipt = fixture.assert_owned(id);
    assert_eq!(receipt["task"]["status"], "failed");
    assert_eq!(receipt["task"]["terminal"], true);
    assert_eq!(receipt["worker"]["outcome"], "completed");
}

#[test]
fn unavailable_model_fails_without_self_heal_or_model_substitution() {
    // PTY stderr joins the task log, where model_selfheal only accepts JSON error envelopes.
    let body = first_attempt(concat!(
        "printf '%s\\n' ",
        "'{\"type\":\"error\",\"message\":\"Error: unknown model caller-model\"}'\nexit 1"
    ));
    let control = Recovery::new(&body);
    control.start("t-model-control", false, &[]);
    control.assert_control("t-model-control", 2, 2);
    assert!(control.has_event("t-model-control", "model unavailable → retry on default"));
    failed_once(&body, &[]);
}

#[test]
fn transient_no_first_output_hang_does_not_retry() {
    // AID_FIRST_TOKEN_TIMEOUT_SECS=1 is applied to both runs by Recovery::command.
    // A silent first attempt must reach the transient watchdog, not the 15s hard cap.
    let body = first_attempt("exec sleep 8");
    let control = Recovery::new(&body);
    control.start("t-hang-control", false, &["--idle-timeout", "1"]);
    control.assert_control("t-hang-control", 2, 2);
    assert!(control.has_event("t-hang-control", "HUNG → retry"));
    let owned = Recovery::new(&body);
    owned.start("t-hang-owned", true, &["--idle-timeout", "1"]);
    assert_eq!(owned.assert_owned("t-hang-owned")["task"]["status"], "failed");
    let events = owned.show("t-hang-owned")["events"].as_array().unwrap().clone();
    assert!(
        events.iter().any(|event| event["metadata"]["transient"] == true
            && event["metadata"]["hung_recovery_eligible"] == true)
    );
}

#[test]
fn quota_error_does_not_continue_on_another_execution() {
    // No custom-only positive control: advice_route::automatic_candidate reads candidates,
    // while shell custom agents appear only in custom_candidates (selection_advice.rs).
    // Built-in provider binaries are excluded from the fixture PATH; no network fallback.
    failed_once(
        concat!(
            "printf '%s\\n' '{\"type\":\"error\",\"message\":",
            "\"429 rate limit exceeded: quota exhausted\"}'\nexit 1"
        ),
        &[],
    );
}

#[test]
fn capacity_error_does_not_continue_on_another_execution() {
    let body = first_attempt(concat!(
        "printf '%s\\n' '{\"type\":\"error\",\"message\":",
        "\"Selected model is at capacity. Please try a different model.\"}'\nexit 1"
    ));
    let control = Recovery::new(&body);
    // A different custom route avoids retrying the model that is now held for capacity.
    let config = std::fs::read_to_string(control.h.path("aid/agents/controlled.toml")).unwrap();
    std::fs::write(
        control.h.path("aid/agents/controlled-peer.toml"),
        config.replace("id = \"controlled\"", "id = \"controlled-peer\""),
    )
    .unwrap();
    control.start("t-capacity-control", false, &["--cascade", "controlled-peer"]);
    control.assert_control("t-capacity-control", 2, 2);
    failed_once(&body, &[]);
}

#[test]
fn failing_verification_does_not_retry() {
    let control = Recovery::new(SUCCESS);
    control.start("t-verify-control", false, &["--verify", "false", "--retry", "1"]);
    control.assert_control("t-verify-control", 2, 2);
    control.assert_child_prompt("t-verify-control", "Verification failed. Please fix");
    // Caller-owned admission refuses retry > 0, so only the control enables it.
    failed_once(SUCCESS, &["--verify", "false"]);
}

#[test]
fn missing_checklist_item_does_not_retry() {
    let item = "report CHECKLIST 1: confirmed";
    let control = Recovery::new(SUCCESS);
    control.start(
        "t-checklist-control",
        false,
        &["--retry", "1", "--checklist", item],
    );
    control.assert_control("t-checklist-control", 2, 2);
    control.assert_child_prompt("t-checklist-control", "[Checklist items not addressed]");
    let owned = Recovery::new(SUCCESS);
    owned.start("t-checklist-owned", true, &["--checklist", item]);
    // The protocol disables checklist retries; it does not prescribe the final task status.
    owned.assert_owned("t-checklist-owned");
}

#[test]
fn dirty_worktree_does_not_launch_a_rescue_agent() {
    let body = format!("printf 'changed\\n' > tracked.txt\n{SUCCESS}");
    for owned in [false, true] {
        let fixture = Recovery::new(&body);
        fixture.h.git_project();
        let id = if owned { "t-dirty-owned" } else { "t-dirty-control" };
        fixture.start(id, owned, &["--worktree", "test/caller-dirty"]);
        if owned {
            assert_eq!(fixture.assert_owned(id)["task"]["status"], "failed");
            assert!(fixture.has_event(id, "FAIL: agent left uncommitted changes"));
        } else {
            fixture.assert_control(id, 1, 1);
            assert!(fixture.has_event(id, "Rescued 1 file(s)"));
        }
        let shown = fixture.show(id);
        let worktree = shown["worktree_path"].as_str().unwrap();
        let subject = fixture.git(worktree, &["log", "-1", "--format=%s"]);
        assert_eq!(subject.contains("[aid] rescue:"), !owned, "{subject}");
        let status = fixture.git(worktree, &["status", "--porcelain", "--", "tracked.txt"]);
        // Also catches failure_salvage committing caller-owned changes after mark_failed.
        assert_eq!(status.contains("tracked.txt"), owned, "{status}");
    }
}
