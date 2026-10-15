// Fake-server coverage through the same PTY dispatch entry point as background workers.
// Covers receipts, refusal, resume, interruption, malformed protocol, and early EOF.

use super::super::*;
#[path = "fixture.rs"]
mod fixture;
use fixture::Fixture;

#[test]
fn full_dispatch_confirms_steer_and_preserves_events_cost_and_output() {
    let fixture = Fixture::new("accept");
    let actor = fixture.actor(false);
    fixture.dispatch();
    actor.join().unwrap();
    let task = fixture
        .store
        .get_task(fixture.task.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Done);
    assert_eq!(task.tokens, Some(44350));
    assert_eq!(task.observed_model.as_deref(), Some("gpt-6.1-sol"));
    assert!(task.agent_session_id.is_some());
    let messages = fixture
        .store
        .list_messages_for_task(fixture.task.as_str())
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].delivered_at.is_some());
    assert!(
        receipt(&fixture.store, fixture.task.as_str(), messages[0].id)
            .unwrap()
            .is_some()
    );
    assert_eq!(std::fs::read_to_string(&fixture.output).unwrap(), "STEERED");
    assert!(!active(fixture.task.as_str()));
    assert!(fixture.home.path().join("codex-home").is_dir());
    assert!(
        !crate::support::paths::task_dir(fixture.task.as_str())
            .join("codex-home")
            .exists()
    );
}

#[test]
fn rejected_and_mismatched_steers_are_never_marked_delivered() {
    for mode in ["reject", "mismatch"] {
        let fixture = Fixture::new(mode);
        let actor = fixture.actor(false);
        fixture.dispatch();
        actor.join().unwrap();
        let messages = fixture
            .store
            .list_messages_for_task(fixture.task.as_str())
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].delivered_at.is_none());
        let error = receipt(&fixture.store, fixture.task.as_str(), messages[0].id).unwrap_err();
        assert!(error.to_string().contains(if mode == "reject" {
            "non-steerable"
        } else {
            "mismatch"
        }));
    }
}

#[test]
fn stop_interrupts_the_active_turn_before_process_termination() {
    let fixture = Fixture::new("interrupt");
    let actor = fixture.actor(true);
    fixture.dispatch();
    actor.join().unwrap();
    assert!(fixture.home.path().join("interrupted").exists());
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Stopped
    );
    assert!(!active(fixture.task.as_str()));
}

#[test]
fn resume_uses_thread_resume_and_keeps_final_output() {
    let mut fixture = Fixture::new("complete");
    let executable = fixture.command.get_program().to_os_string();
    let envs: Vec<_> = fixture
        .command
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(std::ffi::OsStr::to_os_string)))
        .collect();
    let mut command = std::process::Command::new(executable);
    command.args([
        "exec",
        "resume",
        "--json",
        "resume-thread",
        "prompt",
        "--ignore-user-config",
    ]);
    for (key, value) in envs {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    fixture.command = command;
    fixture.dispatch();
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
fn malformed_startup_falls_back_but_missing_terminal_notification_fails() {
    for mode in ["invalid", "eof"] {
        let fixture = Fixture::new(mode);
        fixture.dispatch();
        assert_eq!(
            fixture
                .store
                .get_task(fixture.task.as_str())
                .unwrap()
                .unwrap()
                .status,
            if mode == "invalid" {
                TaskStatus::Done
            } else {
                TaskStatus::Failed
            }
        );
        assert!(
            !crate::support::paths::task_dir(fixture.task.as_str())
                .join("codex-home")
                .exists()
        );
    }
}

#[test]
fn initialize_timeout_is_bounded_and_falls_back_clearly() {
    let fixture = Fixture::new("silent-init");
    let started = std::time::Instant::now();
    fixture.dispatch();
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("initialize response timed out"))
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
fn piped_dispatch_uses_the_same_transport_and_completion_pipeline() {
    let fixture = Fixture::new("complete");
    let _permit = crate::support::test_subprocess::acquire();
    let mut command = tokio::process::Command::new(fixture.command.get_program());
    command.args(fixture.command.get_args());
    for (key, value) in fixture.command.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::cmd::run::run_agent_process_with_cost(
            &crate::agent::codex::CodexAgent,
            command,
            &fixture.task,
            &fixture.store,
            &fixture.log,
            Some(fixture.output.to_str().unwrap()),
            None,
            true,
            None,
            crate::config::timeout_policy::TimeoutPolicy::default(),
            None,
        ))
        .unwrap();
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
fn steer_timeout_never_marks_delivery_even_when_turn_finishes() {
    let fixture = Fixture::new("silent-steer");
    let actor = fixture.actor(false);
    let started = std::time::Instant::now();
    fixture.dispatch();
    actor.join().unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(35));
    let messages = fixture
        .store
        .list_messages_for_task(fixture.task.as_str())
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].delivered_at.is_none());
    let error = receipt(&fixture.store, fixture.task.as_str(), messages[0].id).unwrap_err();
    assert!(error.to_string().contains("acceptance unconfirmed"));
}

#[test]
fn thread_start_timeout_is_bounded_and_falls_back_clearly() {
    let fixture = Fixture::new("silent-thread");
    let started = std::time::Instant::now();
    fixture.dispatch();
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("thread/start response timed out"))
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
fn synchronous_reply_and_steer_wait_for_matching_rpc_acceptance() {
    for mode in ["reply-sync", "steer"] {
        let fixture = Fixture::new("accept");
        let actor = fixture.actor_mode(mode);
        fixture.dispatch();
        actor.join().unwrap();
        let messages = fixture
            .store
            .list_messages_for_task(fixture.task.as_str())
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].delivered_at.is_some());
    }
}
