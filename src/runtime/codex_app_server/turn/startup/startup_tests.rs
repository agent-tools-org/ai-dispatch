// Startup recovery and resume regressions through the real dispatch entry point.
// Slow fake replies exercise the bounded ten-second startup deadline.

use crate::runtime::codex_app_server::*;
#[path = "../fixture.rs"]
mod fixture;
use fixture::Fixture;
use std::time::Duration;

fn policy(seconds: u64) -> crate::config::timeout_policy::TimeoutPolicy {
    let mut policy = crate::config::timeout_policy::TimeoutPolicy::default();
    policy.idle = Duration::from_secs(seconds);
    policy
}

fn fallback_events(fixture: &Fixture) -> Vec<TaskEvent> {
    fixture
        .store
        .get_events(fixture.task.as_str())
        .unwrap()
        .into_iter()
        .filter(|event| {
            event
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata["codex_app_server_fallback"] == true)
        })
        .collect()
}

#[test]
fn startup_falls_back_at_ten_seconds_even_with_a_longer_idle_budget() {
    let fixture = Fixture::new("slow-init");
    fixture.dispatch_with_policy(policy(15));
    assert_eq!(fallback_events(&fixture).len(), 1);
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Done
    );
}

#[test]
fn failed_handshake_falls_back_with_diagnostic_and_original_prompt() {
    for mode in [
        "invalid",
        "silent-init",
        "silent-thread",
        "reject-start",
        "stderr-init",
    ] {
        let fixture = Fixture::new(mode);
        fixture.dispatch_with_policy(policy(1));
        let events = fallback_events(&fixture);
        assert_eq!(events.len(), 1, "{mode}");
        assert_eq!(
            fixture
                .store
                .get_task(fixture.task.as_str())
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Done
        );
        if mode == "stderr-init" {
            assert!(
                events[0].metadata.as_ref().unwrap()["stderr"]
                    .as_str()
                    .unwrap()
                    .contains("synthetic startup diagnostic")
            );
        }
    }
}

#[test]
fn submitted_turn_without_a_response_is_not_replayed_in_exec() {
    let fixture = Fixture::new("silent-turn");
    fixture.dispatch_with_policy(policy(1));
    assert!(fallback_events(&fixture).is_empty());
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn long_resume_times_out_and_launches_exec_resume() {
    let mut fixture = Fixture::new("slow-resume");
    let mut command = std::process::Command::new(fixture.command.get_program());
    command.args([
        "exec",
        "resume",
        "--json",
        "resume-thread",
        "prompt",
        "--ignore-user-config",
    ]);
    for (key, value) in fixture.command.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    fixture.command = command;
    fixture.dispatch_with_policy(policy(15));
    assert_eq!(fallback_events(&fixture).len(), 1);
    assert!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .contains("STEERED")
    );
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Done
    );
}

#[test]
fn startup_fallback_cannot_restart_execution_after_the_hard_deadline() {
    let fixture = Fixture::new("silent-init");
    let mut policy = policy(5);
    policy.max_duration = Duration::from_millis(200);
    fixture.dispatch_with_policy(policy);
    assert!(fallback_events(&fixture).is_empty());
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn unconfirmed_turn_start_failure_keeps_stderr_without_replaying_work() {
    let fixture = Fixture::new("stderr-start");
    fixture.dispatch_with_policy(policy(1));
    assert!(fallback_events(&fixture).is_empty());
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
    assert!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .contains("synthetic turn-start diagnostic")
    );
}
