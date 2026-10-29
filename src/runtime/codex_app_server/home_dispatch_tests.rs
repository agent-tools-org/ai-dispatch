// Mirror precondition failures exercise the production builder and exec dispatch fallback.
// Synthetic homes preserve all local content and require no credentials or model calls.

use super::*;
use crate::agent::{Agent, RunOpts};
use std::process::Command;
use crate::runtime::codex_app_server::fixture::Fixture;

fn assert_exec(fixture: &Fixture, reason: &str) {
    fixture.dispatch();
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.metadata.as_ref().is_some_and(|metadata| {
                metadata["codex_app_server_fallback"] == true
                    && metadata["reason"]
                        .as_str()
                        .is_some_and(|detail| detail.contains(reason))
            }))
    );
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
fn retry_uses_exec_when_real_home_lacks_sessions() {
    let mut fixture = Fixture::new("complete");
    let real = fixture.home.path().join("real-codex");
    std::fs::remove_dir(real.join("sessions")).unwrap();
    let local = fixture.home.path().join("codex-home/sessions");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(
        local.join("rollout-2026-10-15T10-00-00-saved-session.jsonl"),
        "local",
    )
    .unwrap();
    let opts = retry_options();
    crate::agent::codex::cli_config::set_test_codex_home(Some(real));
    let built = crate::agent::codex::CodexAgent.build_command("retry prompt", &opts);
    crate::agent::codex::cli_config::set_test_codex_home(None);
    let built = built.unwrap();
    assert!(!built.get_args().any(|arg| arg == "resume"));
    let mut command = Command::new(fixture.command.get_program());
    command.args(built.get_args());
    for (key, value) in fixture.command.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    fixture.command = command;
    assert_exec(&fixture, "requires existing sessions and state_5.sqlite");
    assert!(local.is_dir());
    assert!(!fixture.home.path().join("real-codex/sessions").exists());
}

fn retry_options() -> RunOpts {
    RunOpts {
        read_only: true,
        session_id: Some("saved-session".into()),
        ..Default::default()
    }
}

#[test]
fn uses_exec_when_real_home_lacks_state_database() {
    let fixture = Fixture::new("complete");
    let database = fixture.home.path().join("real-codex/state_5.sqlite");
    std::fs::remove_file(&database).unwrap();
    assert_exec(&fixture, "requires existing sessions and state_5.sqlite");
    assert!(!database.exists());
}

#[test]
fn refresh_falls_back_when_local_entry_collides_with_real_entry() {
    for directory in [false, true] {
        let fixture = Fixture::new("complete");
        let home = Home::for_command(&fixture.command).unwrap();
        let local = home.path.join("later");
        let real = fixture.home.path().join("real-codex/later");
        if directory {
            std::fs::create_dir(&local).unwrap();
            std::fs::create_dir(&real).unwrap();
        } else {
            std::fs::write(&local, "local").unwrap();
            std::fs::write(&real, "real").unwrap();
        }
        assert_exec(&fixture, "local state invisible to the real home");
        assert!(!local.symlink_metadata().unwrap().file_type().is_symlink());
        if !directory {
            assert_eq!(std::fs::read_to_string(local).unwrap(), "local");
            assert_eq!(std::fs::read_to_string(real).unwrap(), "real");
        }
    }
}

#[test]
fn rejects_non_regular_config_entries_in_persistent_mirror() {
    for name in ["config.toml-dir", "config.toml-link"] {
        let fixture = Fixture::new("complete");
        let home = Home::for_command(&fixture.command).unwrap();
        let path = home.path.join(name);
        if name.ends_with("-dir") {
            std::fs::create_dir(&path).unwrap();
        } else if name.ends_with("-link") {
            std::os::unix::fs::symlink(home.path.join("missing"), &path).unwrap();
        } else {
            std::fs::write(&path, "synthetic user config").unwrap();
        }
        assert_exec(&fixture, "contains non-regular user configuration");
        assert!(path.symlink_metadata().is_ok());
    }
}
