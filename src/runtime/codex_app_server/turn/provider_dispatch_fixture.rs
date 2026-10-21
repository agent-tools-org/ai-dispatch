// Claim provider fixture tasks through production dispatch preparation.
// Builds the native command from persisted identity before fake app-server execution.
use super::*;

pub(super) fn claim(fixture: &Fixture) {
    let mut args = crate::cmd::run::RunArgs {
        agent_name: "codex".into(),
        prompt: "Inspect the fixture read only".into(),
        dir: Some(fixture.home.path().display().to_string()),
        read_only: true,
        existing_task_id: Some(fixture.task.clone()),
        dry_run: true,
        ..Default::default()
    };
    let builds = crate::agent::codex::command_builds::count();
    let id = crate::cmd::run::claim_test_dispatch(&fixture.store, &mut args).unwrap();
    assert_eq!(id, fixture.task);
    assert!(crate::agent::codex::command_builds::count() > builds);
    assert!(fixture.store.update_task_status(
        id.as_str(), crate::types::TaskStatus::Running).unwrap());
}

pub(super) fn command(fixture: &mut Fixture, resume: bool) {
    let opts = options(fixture, resume);
    let task = fixture
        .store
        .get_task(fixture.task.as_str())
        .unwrap()
        .unwrap();
    let native = crate::agent::codex::routed(task.custom_agent_name)
        .build_command_with_context(
            "retry prompt",
            &opts,
            CommandContext {
                durable_codex_home: false,
                writable_roots: vec![],
            },
        )
        .unwrap();
    let mut command = Command::new(fixture.command.get_program());
    command.args(native.get_args());
    command.current_dir(fixture.home.path());
    for source in [&fixture.command, &native] {
        for (key, value) in source.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
    }
    command
        .env("OPENCODE_CONFIG", "synthetic-provider-env")
        .env("PROVIDER_HEADER", "synthetic-header-env")
        .env(
            "AID_FAKE_PROVIDER_CAPTURE",
            fixture.home.path().join("capture.jsonl"),
        );
    crate::agent::env_redirects::strip_inherited_config_redirects(&mut command);
    crate::agent::apply_codex_home_env(&mut command).unwrap();
    fixture.command = command;
}
