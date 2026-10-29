// Caller-owned executions never multiply when automatic recovery triggers fire.
// Exercises model, hang, dirty delivery, quota, verify and checklist failures via CLI.
// Deps: counted shell agent harness and the built aid binary.
#![cfg(unix)]
#[path = "common/caller_owned.rs"]
mod caller_owned;
mod common;
use caller_owned::{Harness, SUCCESS};

fn failed_once(body: &str, extra: &[&str]) {
    let h = Harness::new(body);
    let id = "t-recovery";
    h.launch(id, extra);
    let receipt = h.settled(id);
    assert_eq!(receipt["task"]["status"], "failed");
    assert_eq!(receipt["task"]["terminal"], true);
    assert_eq!(receipt["worker"]["outcome"], "completed");
    h.assert_rows(id, 1);
}

#[test]
fn unavailable_model_fails_without_self_heal_or_model_substitution() {
    // Explicit stderr prefix matches routing/model_health.rs and run/model_selfheal.rs.
    failed_once("printf 'Error: unknown model caller-model\\n' >&2\nexit 1", &[]);
}

#[test]
fn transient_no_first_output_hang_does_not_retry() {
    // No parsed event before idle expiry is the monitor's transient-hang case.
    failed_once("exec sleep 8", &["--idle-timeout", "1"]);
}

#[test]
fn quota_error_does_not_continue_on_another_execution() {
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
    failed_once(
        concat!(
            "printf '%s\\n' '{\"type\":\"error\",\"message\":",
            "\"Selected model is at capacity. Please try a different model.\"}'\nexit 1"
        ),
        &[],
    );
}

#[test]
fn failing_verification_does_not_retry() {
    failed_once(SUCCESS, &["--verify", "false"]);
}

#[test]
fn missing_checklist_item_does_not_retry() {
    let h = Harness::new(SUCCESS);
    let id = "t-checklist";
    h.launch(id, &["--checklist", "report CHECKLIST 1: confirmed"]);
    // The protocol disables checklist retries; it does not prescribe the final task status.
    let receipt = h.settled(id);
    assert_eq!(receipt["task"]["terminal"], true);
    h.assert_rows(id, 1);
}

#[test]
fn dirty_worktree_does_not_launch_a_rescue_agent() {
    let body = format!("printf 'changed\\n' > tracked.txt\n{SUCCESS}");
    let h = Harness::new(&body);
    h.git_project();
    let id = "t-dirty";
    h.launch(id, &["--worktree", "test/caller-dirty"]);
    let receipt = h.settled(id);
    assert_eq!(receipt["task"]["terminal"], true);
    h.assert_rows(id, 1);
}
