// Fake Codex captures provider configuration on exec, app-server and fallback.
// Covers fresh/resumed threads, environment inheritance and isolation provenance.

use super::fixture::Fixture;
use crate::agent::{
    Agent, CommandContext, RunOpts,
    codex::{CodexAgent, cli_config},
};
use serde_json::Value;
use std::process::Command;

const CONFIG: &str = r#"
model_provider = "fixture"
notify = ["excluded-notify"]
sandbox_mode = "danger-full-access"
[model_providers.fixture]
name = "Fixture"
base_url = "https://provider.invalid/v1"
env_key = "OPENCODE_CONFIG"
wire_api = "responses"
env_http_headers = { "X-Auth" = "PROVIDER_HEADER" }
[mcp_servers.excluded]
command = "excluded-mcp"
"#;

struct ConfigGuard;
impl Drop for ConfigGuard {
    fn drop(&mut self) {
        cli_config::set_test_codex_home(None);
    }
}

fn prepare(mode: &str, resume: bool) -> (Fixture, ConfigGuard) {
    let mut fixture = Fixture::new(mode);
    fixture
        .store
        .db()
        .execute(
            "UPDATE tasks SET read_only=1 WHERE id=?1",
            [fixture.task.as_str()],
        )
        .unwrap();
    let home = fixture.home.path().join("real-codex");
    std::fs::write(home.join("config.toml"), CONFIG).unwrap();
    cli_config::set_test_codex_home(Some(home));
    let opts = options(&fixture, resume);
    let native = CodexAgent
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
    (fixture, ConfigGuard)
}

fn options(fixture: &Fixture, resume: bool) -> RunOpts {
    RunOpts {
        dir: Some(fixture.home.path().display().to_string()),
        model: None,
        output: Some(fixture.output.display().to_string()),
        result_file: None,
        budget: false,
        read_only: true,
        sandbox: false,
        context_files: vec![],
        session_id: resume.then(|| "resume-thread".into()),
        env: None,
        env_forward: None,
    }
}

fn captures(fixture: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(fixture.home.path().join("capture.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn overrides(argv: &Value) -> Vec<String> {
    argv.as_array()
        .unwrap()
        .windows(2)
        .filter(|pair| pair[0] == "-c")
        .filter_map(|pair| pair[1].as_str())
        .filter(|setting| setting.starts_with("model_provider"))
        .map(String::from)
        .collect()
}

#[test]
fn fake_server_start_and_resume_match_exec_and_fallback_provider() {
    for resume in [false, true] {
        let (fixture, _config) = prepare("complete", resume);
        assert!(
            crate::runtime::codex_app_server::command::read_only_config(&fixture.command).is_some()
        );
        fixture.dispatch();
        assert!(
            crate::runtime::read_only_launch::enforced(
                &fixture.store,
                &fixture.task,
                fixture.home.path().to_str().unwrap()
            )
            .unwrap()
        );
        let rows = captures(&fixture);
        let start = rows
            .iter()
            .find(|row| {
                row["method"]
                    == if resume {
                        "thread/resume"
                    } else {
                        "thread/start"
                    }
            })
            .unwrap();
        let config = &start["params"]["config"];
        assert_eq!(config["model_provider"], "fixture");
        assert_eq!(config["model_providers.fixture.env_key"], "OPENCODE_CONFIG");
        let server = rows
            .iter()
            .find(|row| row["argv"][0] == "app-server")
            .unwrap();
        assert_eq!(server["provider_env"], "synthetic-provider-env");
        assert_eq!(server["header_env"], "synthetic-header-env");
        assert_exec_parity(&fixture, server, config);
        assert_isolated(&fixture, &rows);
    }
}

fn assert_isolated(fixture: &Fixture, rows: &[Value]) {
    let captured = serde_json::to_string(rows).unwrap();
    assert!(!captured.contains("excluded-notify"));
    assert!(!captured.contains("excluded-mcp"));
    assert!(!fixture.home.path().join("codex-home/config.toml").exists());
}

fn assert_exec_parity(fixture: &Fixture, server: &Value, config: &Value) {
    let mut fallback = crate::runtime::codex_app_server::exec_fallback(
        &fixture.command,
        &fixture.store,
        &fixture.task,
    )
    .unwrap();
    let real_home = fixture.home.path().join("real-codex");
    assert!(
        fallback
            .get_envs()
            .any(|(key, value)| { key == "CODEX_HOME" && value == Some(real_home.as_os_str()) })
    );
    let output = fallback.output().expect("launch fake exec fallback");
    assert!(
        output.status.success(),
        "fake exec fallback failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = captures(fixture);
    let exec = rows.iter().find(|row| row["argv"][0] == "exec").unwrap();
    assert_eq!(overrides(&server["argv"]), overrides(&exec["argv"]));
    for setting in overrides(&exec["argv"]) {
        let (key, raw) = setting.split_once('=').unwrap();
        let table: toml::Table = toml::from_str(&format!("value={raw}")).unwrap();
        assert_eq!(config[key], serde_json::to_value(&table["value"]).unwrap());
    }
    assert_eq!(exec["provider_env"], "synthetic-provider-env");
    assert_eq!(exec["header_env"], "synthetic-header-env");
}

#[test]
fn inline_secret_refuses_command_before_any_child_or_persisted_diagnostic() {
    let fixture = Fixture::new("complete");
    let home = fixture.home.path().join("real-codex");
    std::fs::write(
        home.join("config.toml"),
        concat!(
            "model_provider='fixture'\n[model_providers.fixture]\n",
            "experimental_bearer_token='synthetic-inline-secret'"
        ),
    )
    .unwrap();
    cli_config::set_test_codex_home(Some(home));
    let _guard = ConfigGuard;
    let opts: RunOpts = RunOpts {
        dir: None,
        output: None,
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
    let error = CodexAgent.build_command("prompt", &opts).unwrap_err();
    assert!(error.to_string().contains("env_key / env_http_headers"));
    assert!(!format!("{error:#}").contains("synthetic-inline-secret"));
    assert!(!fixture.home.path().join("capture.jsonl").exists());
    assert!(!fixture.log.exists());
    let events = fixture.store.get_events(fixture.task.as_str()).unwrap();
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("synthetic-inline-secret")
    );
}

#[test]
fn startup_failure_and_refused_resume_keep_provider_on_automatic_exec_fallback() {
    for (mode, resume) in [("invalid", false), ("reject-resume", true)] {
        let (fixture, _config) = prepare(mode, resume);
        fixture.dispatch();
        let rows = captures(&fixture);
        let server = rows
            .iter()
            .find(|row| row["argv"][0] == "app-server")
            .unwrap();
        let exec = rows.iter().find(|row| row["argv"][0] == "exec").unwrap();
        assert_eq!(overrides(&server["argv"]), overrides(&exec["argv"]));
        assert_eq!(exec["provider_env"], "synthetic-provider-env");
        assert!(
            crate::runtime::read_only_launch::enforced(
                &fixture.store,
                &fixture.task,
                fixture.home.path().to_str().unwrap()
            )
            .unwrap()
        );
    }
}

#[test]
fn no_provider_and_openai_produce_identical_native_argv() {
    let (fixture, _config) = prepare("complete", false);
    let path = fixture.home.path().join("real-codex/config.toml");
    std::fs::write(&path, "").unwrap();
    let opts = options(&fixture, false);
    let default = CodexAgent.build_command("prompt", &opts).unwrap();
    std::fs::write(path, "model_provider='openai'").unwrap();
    let openai = CodexAgent.build_command("prompt", &opts).unwrap();
    assert_eq!(
        default.get_args().collect::<Vec<_>>(),
        openai.get_args().collect::<Vec<_>>()
    );
    assert!(
        !openai
            .get_args()
            .any(|arg| arg.to_string_lossy().starts_with("model_provider"))
    );
}
