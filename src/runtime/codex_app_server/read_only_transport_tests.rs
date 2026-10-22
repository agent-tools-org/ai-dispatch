// Native provenance wiring through synthetic app-server and exec fallback launches.
// The fake CLI checks transport behavior; native OS enforcement has its own policy probe.

use super::*;
use crate::agent::{CommandContext, RunOpts, codex::CodexAgent};
use crate::runtime::codex_app_server::fixture::Fixture;

fn read_only_fixture(mode: &str) -> Fixture {
    let fixture = Fixture::new(mode);
    fixture
        .store
        .db()
        .execute(
            "UPDATE tasks SET read_only = 1 WHERE id = ?1",
            [fixture.task.as_str()],
        )
        .unwrap();
    fixture
}

fn native_command(fixture: &Fixture, resumed: bool) -> std::process::Command {
    let repo = fixture.home.path().join("checkout");
    let scratch = fixture.home.path().join("staging");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&scratch).unwrap();
    let native = CodexAgent
        .build_command_with_context(
            "retry prompt",
            &RunOpts {
                dir: Some(repo.display().to_string()),
                output: Some(fixture.output.display().to_string()),
                result_file: None,
                model: None,
                budget: false,
                read_only: true,
                sandbox: false,
                context_files: vec![],
                session_id: resumed.then(|| "resume-thread".into()),
                env: None,
                env_forward: None,
            },
            CommandContext {
                durable_codex_home: false,
                writable_roots: vec![scratch],
            },
        )
        .unwrap();
    let mut command = std::process::Command::new(fixture.command.get_program());
    command.args(native.get_args()).current_dir(repo);
    for source in [&fixture.command, &native] {
        for (key, value) in source.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
    }
    command
}

#[test]
fn native_read_only_provenance_is_recorded_on_app_server_fresh_and_resume() {
    for resumed in [false, true] {
        let mut fixture = read_only_fixture("complete");
        fixture.command = native_command(&fixture, resumed);
        fixture.dispatch();
        assert_provenance(&fixture, "app-server");
    }
}

#[test]
fn native_read_only_provenance_is_recorded_on_exec_fallback_and_refused_resume() {
    for (mode, resumed) in [("invalid", false), ("reject-resume", true)] {
        let mut fixture = read_only_fixture(mode);
        fixture.command = native_command(&fixture, resumed);
        fixture.dispatch();
        assert_provenance(&fixture, "exec");
    }
}

fn assert_provenance(fixture: &Fixture, transport: &str) {
    let record = fixture
        .store
        .read_only_launch(fixture.task.as_str())
        .unwrap()
        .unwrap();
    let launch: serde_json::Value = serde_json::from_str(&record).unwrap();
    assert_eq!(launch["transport"], transport);
    assert_eq!(launch["profile"]["identity"], "aid-read-only");
    assert_eq!(launch["profile"]["approval_policy"], "never");
    assert_eq!(
        launch["profile"]["writable_roots"],
        serde_json::json!([fixture.home.path().join("staging").canonicalize().unwrap(),])
    );
    assert!(
        crate::runtime::read_only_launch::enforced(
            &fixture.store,
            &fixture.task,
            fixture.command.get_current_dir().unwrap().to_str().unwrap()
        )
        .unwrap()
    );
}
