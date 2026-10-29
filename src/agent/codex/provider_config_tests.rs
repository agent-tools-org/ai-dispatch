// Built-in provider selection, safe query parameters and hermetic configuration reads.
// All user configuration inputs are explicit temporary files.
use super::*;

#[test]
fn builtins_without_tables_emit_selection_only() {
    for id in ["ollama", "lmstudio", "oss"] {
        let _home = tests::HomeGuard::new(&format!("model_provider='{id}'"));
        let provider = selected().unwrap().unwrap();
        let mut command = Command::new("codex");
        provider.apply(&mut command);
        let args: Vec<_> = crate::support::test_command::args(&command);
        assert_eq!(
            args,
            vec!["-c".to_string(), format!("model_provider=\"{id}\"")]
        );
    }
}

#[test]
fn azure_query_parameters_are_preserved_but_literal_credentials_are_refused() {
    let _home = tests::HomeGuard::new(concat!(
        "model_provider='azure'\n[model_providers.azure]\n",
        "base_url='https://azure.invalid'\nquery_params={api-version='2026-01-01'}"
    ));
    let provider = selected().unwrap().unwrap();
    assert_eq!(
        provider.settings["query_params"]["api-version"].as_str(),
        Some("2026-01-01")
    );
    assert!(read_only_override(
        "model_providers.azure.query_params",
        &serde_json::json!({"api-version":"2026-01-01"})
    ));
    let _secret = tests::HomeGuard::new(concat!(
        "model_provider='azure'\n[model_providers.azure]\n",
        "query_params={api_key='synthetic-private-value'}"
    ));
    let error = selected().err().unwrap().to_string();
    assert!(error.contains("credentials"));
    assert!(!error.contains("synthetic-private-value"));
}

#[test]
fn malformed_config_names_file_without_source_excerpt_and_unknown_key_names_real_cause() {
    let _home = tests::HomeGuard::new("model_provider=synthetic-private-value");
    let error = selected().err().unwrap().to_string();
    assert!(error.contains("Parse error"));
    assert!(error.contains("config.toml"));
    assert!(!error.contains("synthetic-private-value"));
    let _unknown = tests::HomeGuard::new("model_provider='x'\n[model_providers.x]\nunsupported=5");
    assert!(
        selected()
            .err()
            .unwrap()
            .to_string()
            .contains("Unsupported Codex provider setting")
    );
}

#[test]
fn default_test_home_is_empty_on_other_threads_even_with_an_explicit_home_here() {
    let _home = tests::HomeGuard::new("model_provider='ollama'");
    assert!(selected().unwrap().is_some());
    std::thread::spawn(|| {
        assert!(selected().unwrap().is_none());
        assert!(
            !super::super::cli_config::codex_home()
                .unwrap()
                .join("config.toml")
                .exists()
        );
    })
    .join()
    .unwrap();
}

#[test]
fn environment_header_names_and_variable_names_are_not_inline_credentials() {
    let _home = tests::HomeGuard::new(concat!(
        "model_provider='x'\n[model_providers.x]\n",
        "env_key='SK_API_KEY'\nenv_http_headers={Authorization='SK_HEADER_TOKEN'}"
    ));
    let provider = selected().unwrap().unwrap();
    assert_eq!(provider.settings["env_key"].as_str(), Some("SK_API_KEY"));
    assert!(safe_setting(
        "env_http_headers",
        &provider.settings["env_http_headers"]
    ));
}

#[test]
fn malformed_config_without_top_level_provider_keeps_default() {
    for config in ["bad=[", "[profiles.x]\nmodel_provider='ollama'\nbad=["] {
        let _home = tests::HomeGuard::new(config);
        assert!(selected().expect("default after warning").is_none());
    }
    for config in [
        "model_provider='ollama'\nbad=[",
        "bad=[\nmodel_provider='ollama'",
        "\"model_provider\"='ollama'\nbad=[",
    ] {
        let _home = tests::HomeGuard::new(config);
        let error = selected().expect_err("provider parse error").to_string();
        assert!(error.contains("Parse error"));
        assert!(error.contains("config.toml"));
    }
}

#[test]
fn invalid_provider_shapes_name_the_real_cause() {
    for (config, cause) in [
        ("model_provider=7", "model_provider must be a string"),
        (
            "model_provider='x'\nmodel_providers=7",
            "model_providers must be a table",
        ),
        (
            "model_provider='x'\n[model_providers]\nx=7",
            "provider must be a table",
        ),
    ] {
        let _home = tests::HomeGuard::new(config);
        assert!(
            selected()
                .expect_err("shape error")
                .to_string()
                .contains(cause)
        );
    }
}

#[test]
fn every_carried_setting_is_accepted_by_read_only_provenance() {
    let _home = tests::HomeGuard::new(concat!(
        "model_provider='fixture'\n[model_providers.fixture]\n",
        "name='Fixture'\nbase_url='https://provider.invalid/v1'\n",
        "wire_api='responses'\nenv_key='PROVIDER_KEY'\nenv_key_instructions='Set the environment'\n",
        "env_http_headers={Authorization='PROVIDER_HEADER'}\n",
        "query_params={api-version='2026-01-01'}\nrequest_max_retries=2\n",
        "stream_max_retries=3\nstream_idle_timeout_ms=60000\n",
        "requires_openai_auth=false\nsupports_websockets=true\n"
    ));
    let provider = selected().expect("valid config").expect("selected");
    assert_eq!(provider.settings.len(), 12);
    for (key, value) in provider.settings {
        assert!(read_only_override(
            &format!("model_providers.fixture.{key}"),
            &serde_json::to_value(value).expect("JSON setting")
        ));
    }
}

#[test]
fn route_names_preserve_prefix_recognition_without_validating_ids() {
    for id in ["X", "azure-1_2", "", "../X", "X/Y", "X.Y"] {
        let name = route_for_id(id);
        assert_eq!(name, format!("codex-provider:{id}"));
        assert_eq!(route_id(&name), Some(id));
    }
    for name in ["codex", "Codex-provider:X", "rate-limit-codex-provider:X", "other:X"] {
        assert_eq!(route_id(name), None);
    }
    let _home = tests::HomeGuard::new("model_provider='X'");
    assert_eq!(route_name().unwrap().as_deref(), Some("codex-provider:X"));
}
