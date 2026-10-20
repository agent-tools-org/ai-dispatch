// Both execution paths launch production-built commands against synthetic Codex state.
// The fake server rewrites mirror config exactly where later launches must discard it.

use super::*;
use crate::agent::{Agent, RunOpts};
use std::process::Command;
#[path = "turn/fixture.rs"]
mod fixture;
use fixture::Fixture;

fn build(fixture: &mut Fixture) {
    let opts = RunOpts {
        dir: None,
        output: Some(fixture.output.to_string_lossy().into_owned()),
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    crate::agent::codex::cli_config::set_test_codex_home(Some(
        fixture.home.path().join("real-codex"),
    ));
    let built = crate::agent::codex::CodexAgent.build_command("launch prompt", &opts);
    crate::agent::codex::cli_config::set_test_codex_home(None);
    let built = built.unwrap();
    let mut command = Command::new(fixture.command.get_program());
    command.args(built.get_args());
    for (key, value) in fixture.command.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    fixture.command = command;
}

async fn dispatch(fixture: &Fixture, foreground: bool) {
    if !foreground {
        fixture.dispatch();
        return;
    }
    let _permit = crate::support::test_subprocess::acquire();
    crate::cmd::run::run_agent_process_with_cost(
        &crate::agent::codex::CodexAgent,
        foreground_command(&fixture.command),
        &fixture.task,
        &fixture.store,
        &fixture.log,
        Some(fixture.output.to_str().unwrap()),
        None,
        true,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
}

fn foreground_command(command: &Command) -> tokio::process::Command {
    let mut child = tokio::process::Command::new(command.get_program());
    child.args(command.get_args());
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            child.env(key, value);
        }
    }
    child
}

fn fallback(fixture: &Fixture) -> bool {
    fixture
        .store
        .get_events(fixture.task.as_str())
        .unwrap()
        .iter()
        .any(|event| {
            event
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata["codex_app_server_fallback"] == true)
        })
}

fn assert_done(fixture: &Fixture) {
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

fn next_task(fixture: &mut Fixture) {
    fixture.task = crate::types::TaskId("t-second-launch".into());
    fixture
        .store
        .insert_waiting_task(
            fixture.task.as_str(),
            "codex",
            "launch prompt",
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
    fixture.log = crate::support::paths::log_path(fixture.task.as_str());
}

#[tokio::test]
async fn second_launch_after_codex_writes_mirror_config_still_uses_app_server() {
    for foreground in [false, true] {
        let mut fixture = Fixture::new("write-config");
        build(&mut fixture);
        let real_config = fixture.home.path().join("real-codex/config.toml");
        let before = std::fs::read(&real_config).unwrap();
        dispatch(&fixture, foreground).await;
        assert_done(&fixture);
        assert!(!fallback(&fixture));
        assert!(fixture.home.path().join("codex-home/config.toml").is_file());
        next_task(&mut fixture);
        build(&mut fixture);
        dispatch(&fixture, foreground).await;
        assert_done(&fixture);
        assert!(!fallback(&fixture));
        assert_eq!(std::fs::read(real_config).unwrap(), before);
    }
}

#[tokio::test]
async fn local_non_symlink_entry_falls_back_to_exec() {
    for foreground in [false, true] {
        let mut fixture = Fixture::new("complete");
        build(&mut fixture);
        let home = Home::for_command(&fixture.command).unwrap();
        let local = home.path.join("state_6.sqlite");
        std::fs::write(&local, "unshared state").unwrap();
        dispatch(&fixture, foreground).await;
        assert_done(&fixture);
        assert!(fallback(&fixture));
        assert_eq!(std::fs::read_to_string(local).unwrap(), "unshared state");
        assert!(
            !fixture
                .home
                .path()
                .join("real-codex/state_6.sqlite")
                .exists()
        );
    }
}
