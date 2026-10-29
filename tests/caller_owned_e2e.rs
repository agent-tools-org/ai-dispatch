// Acceptance tests for aid.execution/1 discovery and pre-launch refusals.
// Uses the built CLI and a counted shell custom agent, with no provider requests.
// Deps: tests/common/caller_owned.rs and serde_json.
#![cfg(unix)]
#[path = "common/caller_owned.rs"]
mod caller_owned;
mod common;
use caller_owned::{Harness, PROTOCOL, SUCCESS, json};

#[test]
fn capabilities_are_read_only_and_advertise_the_frozen_policy() {
    let h = Harness::new(SUCCESS);
    let value = json(h.aid().args(["capabilities", "--json"]), 0);
    assert_eq!(value["protocol"], PROTOCOL);
    assert!(value["aid_version"].as_str().is_some_and(|s| !s.is_empty()));
    assert_eq!(
        value["capabilities"],
        serde_json::json!([
            "caller_owned_policy",
            "settlement_receipt",
            "worker_stop",
            "idempotent_submit"
        ])
    );
    let policy = &value["policies"]["caller-owned"];
    assert_eq!(policy["version"], 1);
    assert_eq!(policy["requires"], serde_json::json!(["id", "model"]));
    assert_eq!(
        policy["refused_flags"],
        serde_json::json!([
            "retry",
            "cascade",
            "best_of",
            "iterate",
            "judge",
            "peer_review",
            "audit",
            "budget",
            "dry_run",
            "container",
            "on_done",
            "hook"
        ])
    );
    assert_disabled_recoveries(policy);
    h.assert_empty("t-discovery");
}

fn assert_disabled_recoveries(policy: &serde_json::Value) {
    assert_eq!(
        policy["disabled"],
        serde_json::json!([
            "retry",
            "cascade",
            "model_substitution",
            "route_substitution",
            "model_self_heal",
            "hang_retry",
            "delivery_recovery",
            "dirty_rescue",
            "quota_continuation",
            "verify_retry",
            "checklist_retry",
            "iterate",
            "best_of",
            "judge",
            "peer_review",
            "audit",
            "nested_delegation",
            "transport_fallback",
            "explicit_retry"
        ])
    );
}

fn refused(extra: &[&str], code: &str, detail: Option<&str>, id: bool, model: bool) {
    let h = Harness::new(SUCCESS);
    let mut cmd = h.aid();
    cmd.args(["run", "controlled", "refuse before launch", "--json"]);
    if id {
        cmd.args(["--id", "t-refused"]);
    }
    if model {
        cmd.args(["--model", caller_owned::MODEL]);
    }
    cmd.args(extra);
    let value = json(cmd, 3);
    assert_eq!(value["protocol"], PROTOCOL);
    assert_eq!(value["error"]["code"], code);
    assert!(value["error"]["message"].as_str().is_some_and(|s| !s.is_empty()));
    if let Some(detail) = detail {
        assert_eq!(value["error"]["detail"], detail);
    }
    h.assert_empty("t-refused");
}

#[test]
fn unknown_policy_is_refused_without_a_task() {
    refused(&["--policy", "unknown"], "policy_unsupported", None, true, true);
}
#[test]
fn missing_id_is_refused_without_a_task() {
    refused(&["--policy", "caller-owned"], "id_required", None, false, true);
}
#[test]
fn missing_model_is_refused_without_a_task() {
    refused(&["--policy", "caller-owned"], "model_required", None, true, false);
}

macro_rules! refusal {
    ($name:ident, $detail:literal, $($arg:literal),+) => {
        #[test]
        fn $name() {
            refused(&["--policy", "caller-owned", $($arg),+],
                "flag_refused", Some($detail), true, true);
        }
    };
}
refusal!(retry_is_refused, "retry", "--retry", "1");
refusal!(cascade_is_refused, "cascade", "--cascade", "controlled");
refusal!(best_of_is_refused, "best_of", "--best-of", "2");
refusal!(iterate_is_refused, "iterate", "--iterate", "2");
refusal!(judge_is_refused, "judge", "--judge");
refusal!(
    peer_review_is_refused,
    "peer_review",
    "--peer-review",
    "controlled"
);
refusal!(audit_is_refused, "audit", "--audit");
refusal!(budget_is_refused, "budget", "--budget", "standard");

refusal!(dry_run_is_refused, "dry_run", "--dry-run");
refusal!(container_is_refused, "container", "--container", "fixture-image");
refusal!(on_done_is_refused, "on_done", "--on-done", "true");
refusal!(hook_is_refused, "hook", "--hook", "on_done:true");
