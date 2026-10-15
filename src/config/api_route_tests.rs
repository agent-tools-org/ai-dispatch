// API route configuration defaults and rejection cases.
// Uses the same AidConfig deserializer as normal dispatch.
use super::*;
use crate::config::AidConfig;

fn document(extra: &str) -> String {
    format!(
        "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n\
        keychain_service = 'aid-anthropic-api'\nmonthly_budget_usd = 200\n{extra}"
    )
}

#[test]
fn defaults_and_explicit_account_and_bill() {
    let config: AidConfig = crate::config::parse_config(&document("")).unwrap();
    let route = &config.api_route[0];
    assert_eq!(route.account(), "claude-api-ming");
    assert_eq!(route.over_budget, OverBudget::Stop);
    let config: AidConfig = crate::config::parse_config(&document(
        "keychain_account = 'account-ming'\nover_budget = 'bill'",
    ))
    .unwrap();
    assert_eq!(config.api_route[0].account(), "account-ming");
    assert_eq!(config.api_route[0].over_budget, OverBudget::Bill);
}

#[test]
fn rejects_invalid_routes() {
    for invalid in [
        document("").replace("agent = 'claude'", "agent = 'codex'"),
        document("").replace("200", "-1"),
        document("").replace("200", "nan"),
        document("").replace("200", "0"),
        document("").replace("claude-api-ming", "../bad"),
        document("").replace("claude-api-ming", "claude"),
        document("over_budget = 'ignore'"),
        document("keychain_account = ''"),
        document("").replace("aid-anthropic-api", ""),
        format!("{}\n{}", document(""), document("")),
    ] {
        assert!(crate::config::parse_config(&invalid).is_err());
    }
    let error = crate::config::parse_config(&document("").replace("agent = 'claude'", "agent = 'codex'"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("only agent = \"claude\" is supported"));
}

#[test]
fn keychain_identifiers_reject_shell_syntax_at_config_load() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    for field in ["keychain_service", "keychain_account"] {
        for value in [
            "",
            "has space",
            "'quote",
            "\"quote",
            ";id",
            "$(id)",
            "`id`",
            "line\nbreak",
        ] {
            let mut config: toml::Value = toml::from_str(&document("")).unwrap();
            config["api_route"][0]
                .as_table_mut()
                .expect("route table")
                .insert(field.into(), toml::Value::String(value.into()));
            std::fs::write(
                crate::support::paths::config_path(),
                toml::to_string(&config).unwrap(),
            )
            .unwrap();
            let error = crate::config::load_config().unwrap_err();
            assert!(format!("{error:#}").contains(&format!("{field} must match [A-Za-z0-9._-]+")));
        }
    }
}

#[test]
fn keychain_identifiers_accept_only_safe_word_characters() {
    let config = crate::config::parse_config(&document("keychain_account = 'Account_1.test-meet'")).unwrap();
    assert_eq!(
        config.api_route[0].helper_command().unwrap(),
        "/usr/bin/security find-generic-password -s aid-anthropic-api -a Account_1.test-meet -w"
    );
}

#[test]
fn unsupported_over_budget_numeric_value_is_not_a_default() {
    assert!(crate::config::parse_config(&document("over_budget = 0")).is_err());
}
