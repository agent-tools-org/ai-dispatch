// Provider parsing and launch-boundary regression tests without live Codex calls.
// Uses thread-local Codex homes; synthetic credentials never leave error assertions.

use super::*;
use crate::types::{AgentKind, provider_for_cli};

pub(crate) struct HomeGuard {
    _home: tempfile::TempDir,
}

impl HomeGuard {
    pub(crate) fn new(config: &str) -> Self {
        let home = tempfile::tempdir().expect("home");
        std::fs::write(home.path().join("config.toml"), config).expect("config");
        super::super::cli_config::set_test_codex_home(Some(home.path().into()));
        Self { _home: home }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        super::super::cli_config::set_test_codex_home(None);
    }
}

const CONFIG: &str = r#"
model_provider = "fixture"
notify = ["must-not-launch"]
sandbox_mode = "danger-full-access"
[model_providers.fixture]
name = "Fixture"
base_url = "https://provider.invalid/v1"
env_key = "OPENCODE_CONFIG"
wire_api = "responses"
env_http_headers = { "X-Auth" = "PROVIDER_HEADER" }
[model_providers.other]
experimental_bearer_token = "unused-fixture"
[mcp_servers.unwanted]
command = "must-not-launch"
[profiles.other]
model_provider = "other"
"#;

#[test]
fn only_selected_provider_is_carried_as_overrides() {
    let _home = HomeGuard::new(CONFIG);
    let provider = selected().expect("config").expect("provider");
    let mut command = Command::new("codex");
    provider.apply(&mut command);
    let args: Vec<_> = command.get_args().map(|s| s.to_string_lossy()).collect();
    assert!(args.iter().any(|s| s == "model_provider=\"fixture\""));
    assert_eq!(args.len(), 12);
    assert!(!format!("{args:?}").contains("must-not-launch"));
    assert!(!format!("{args:?}").contains("unused-fixture"));
    crate::agent::env_redirects::strip_inherited_config_redirects(&mut command);
    assert!(
        !command
            .get_envs()
            .any(|(key, value)| key == "OPENCODE_CONFIG" && value.is_none())
    );
    let mut names = referenced_env(&command);
    names.sort();
    assert_eq!(names, vec!["OPENCODE_CONFIG", "PROVIDER_HEADER"]);
}

#[test]
fn absent_and_openai_provider_leave_overrides_empty() {
    for text in [
        "",
        "model_provider='openai'",
        "[profiles.x]\nmodel_provider='fixture'",
    ] {
        let _home = HomeGuard::new(text);
        assert!(selected().expect("default").is_none());
        assert_eq!(
            provider_for_cli(AgentKind::Codex).0.as_str(),
            "openai-chatgpt-plan"
        );
    }
}

#[test]
fn rejects_inline_credentials_without_echoing_values_or_source() {
    for field in [
        "experimental_bearer_token='fixture-secret'",
        "http_headers={Auth='fixture-secret'}",
        "api_key='fixture-secret'",
        "name='Bearer fixture-secret'",
        "env_key_instructions='sk-fixture-secret'",
        "base_url='https://fixture-secret@provider.invalid'",
        "base_url='https://provider.invalid?key=fixture-secret'",
    ] {
        let text = format!("model_provider='fixture'\n[model_providers.fixture]\n{field}");
        let _home = HomeGuard::new(&text);
        let error = selected().err().expect("must refuse").to_string();
        assert!(error.contains("env_key / env_http_headers"));
        assert!(!error.contains("fixture-secret"));
    }
    let _home = HomeGuard::new("model_provider='fixture'\nbad=fixture-secret");
    assert!(
        !selected()
            .err()
            .expect("invalid TOML")
            .to_string()
            .contains("fixture-secret")
    );
}

#[test]
fn provider_cannot_override_permissions_in_read_only_provenance() {
    assert!(read_only_override(
        "model_provider",
        &serde_json::json!("fixture")
    ));
    assert!(read_only_override(
        "model_providers.fixture.env_key",
        &serde_json::json!("KEY")
    ));
    for key in [
        "model_providers.fixture.permissions",
        "model_providers.fixture.hooks",
        "model_providers.fixture.http_headers",
        "model_providers.fixture.extra.base_url",
    ] {
        assert!(!read_only_override(key, &serde_json::json!("bad")));
    }
}

#[test]
fn unsafe_fields_and_isolation_environment_references_fail_closed() {
    for field in [
        "env_key='CODEX_HOME'",
        "env_http_headers={Auth='HOME'}",
        "env_key='literal credential'",
        "hooks=[]",
        "request_max_retries='secret'",
        "stream_max_retries=-1",
        "requires_openai_auth='yes'",
        "query_params={key='secret'}",
    ] {
        let text = format!("model_provider='fixture'\n[model_providers.fixture]\n{field}");
        let _home = HomeGuard::new(&text);
        assert!(selected().is_err());
    }
    let _home = HomeGuard::new(
        "model_provider='openai'\n[model_providers.openai]\nhttp_headers={Auth='secret'}",
    );
    assert!(selected().is_err());
    let mut other = Command::new("claude");
    other.args(["-c", "model_providers.fixture.env_key='OPENCODE_CONFIG'"]);
    crate::agent::env_redirects::strip_inherited_config_redirects(&mut other);
    assert!(
        other
            .get_envs()
            .any(|(key, value)| key == "OPENCODE_CONFIG" && value.is_none())
    );
}

#[test]
fn custom_provider_keeps_existing_codex_route_identity() {
    let default = crate::types::Route::for_cli(AgentKind::Codex);
    let _home = HomeGuard::new(CONFIG);
    assert_eq!(crate::types::Route::for_cli(AgentKind::Codex), default);
}
