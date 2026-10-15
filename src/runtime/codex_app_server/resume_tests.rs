// Persistent-home resume and exec fallback regressions using synthetic task state.
// No Codex credentials, shared database edits, or model calls are involved.

use super::*;
#[path = "turn/fixture.rs"]
mod fixture;
use fixture::Fixture;

fn retry(fixture: &mut Fixture) {
    let session = fixture
        .store
        .get_task(fixture.task.as_str())
        .unwrap()
        .unwrap()
        .agent_session_id
        .unwrap();
    fixture.task = TaskId(format!("{}-retry", fixture.task.as_str()));
    fixture
        .store
        .insert_waiting_task(
            fixture.task.as_str(),
            "codex",
            "retry prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    fixture
        .store
        .db()
        .execute(
            "UPDATE tasks SET status='running' WHERE id=?1",
            [fixture.task.as_str()],
        )
        .unwrap();
    fixture.command = retry_command(fixture, &session);
    fixture.log = crate::support::paths::log_path(fixture.task.as_str());
}

fn retry_command(fixture: &Fixture, session: &str) -> Command {
    let mut command = Command::new(fixture.command.get_program());
    command.args([
        "exec",
        "resume",
        "--json",
        session,
        "retry prompt",
        "--ignore-user-config",
    ]);
    command.arg("-o").arg(&fixture.output);
    for (key, value) in fixture.command.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    command
}

#[test]
fn resume_survives_process_exit_and_task_state_removal() {
    let mut fixture = Fixture::new("persist-origin");
    fixture
        .command
        .env("AID_FAKE_ORIGIN", fixture.home.path().join("origin"));
    fixture.dispatch();
    let task_dir = crate::support::paths::task_dir(fixture.task.as_str());
    assert!(!task_dir.join("codex-app-server.json").exists());
    std::fs::remove_dir_all(task_dir).unwrap();
    assert!(fixture.home.path().join("codex-home").is_dir());
    retry(&mut fixture);
    fixture.dispatch();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        crate::types::TaskStatus::Done
    );
    assert!(
        !fixture
            .store
            .get_events(fixture.task.as_str())
            .unwrap()
            .iter()
            .any(|event| event.detail.contains("falling back"))
    );
}

#[test]
fn rejected_resume_dispatches_fresh_exec_with_retry_prompt() {
    let mut fixture = Fixture::new("complete");
    fixture.dispatch();
    retry(&mut fixture);
    fixture.command.env("AID_FAKE_MODE", "reject-resume");
    fixture.dispatch();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        crate::types::TaskStatus::Done
    );
    assert!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .contains("STEERED")
    );
    assert!(
        fixture
            .store
            .get_events(fixture.task.as_str())
            .unwrap()
            .iter()
            .any(|event| event.detail.contains("exec with a fresh session"))
    );
}

#[test]
fn handshake_failure_preserves_exec_resume_with_stable_home() {
    let mut fixture = Fixture::new("persist-origin");
    fixture
        .command
        .env("AID_FAKE_ORIGIN", fixture.home.path().join("origin"));
    fixture.dispatch();
    retry(&mut fixture);
    fixture.command.env("AID_FAKE_MODE", "stderr-init");
    fixture.dispatch();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        crate::types::TaskStatus::Done
    );
    assert!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .contains("STEERED")
    );
}

#[test]
fn fresh_exec_preserves_options_literal_prompt_directory_and_environment() {
    let mut cmd = Command::new("codex");
    cmd.args([
        "exec",
        "resume",
        "-c",
        "model=x",
        "--json",
        "session",
        "literal\n{\"id\":1}",
    ]);
    cmd.current_dir("/tmp")
        .env("KEPT", "yes")
        .env_remove("REMOVED");
    let fresh = clone_command(&cmd, true);
    assert_eq!(
        fresh.get_args().collect::<Vec<_>>(),
        ["exec", "-c", "model=x", "--json", "literal\n{\"id\":1}"]
    );
    assert_eq!(fresh.get_current_dir(), cmd.get_current_dir());
    assert_eq!(
        fresh.get_envs().collect::<Vec<_>>(),
        cmd.get_envs().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn foreground_rejected_resume_also_dispatches_fresh_exec() {
    let mut fixture = Fixture::new("complete");
    fixture.dispatch();
    retry(&mut fixture);
    fixture.command.env("AID_FAKE_MODE", "reject-resume");
    let _permit = crate::support::test_subprocess::acquire();
    crate::cmd::run::run_agent_process_with_cost(
        &crate::agent::codex::CodexAgent,
        tokio::process::Command::from(clone_command(&fixture.command, false)),
        &fixture.task,
        &fixture.store,
        &fixture.log,
        Some(fixture.output.to_str().unwrap()),
        None,
        true,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        fixture
            .store
            .get_task(fixture.task.as_str())
            .unwrap()
            .unwrap()
            .status,
        crate::types::TaskStatus::Done
    );
    assert!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .contains("STEERED")
    );
}
