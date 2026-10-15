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
    let config: AidConfig = toml::from_str(&document("")).unwrap();
    let route = &config.api_route[0];
    assert_eq!(route.item().account, "claude-api-ming");
    assert_eq!(route.over_budget, OverBudget::Stop);
    let config: AidConfig = toml::from_str(&document(
        "keychain_account = 'account-ming'\nover_budget = 'bill'",
    ))
    .unwrap();
    assert_eq!(config.api_route[0].item().account, "account-ming");
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
        assert!(toml::from_str::<AidConfig>(&invalid).is_err());
    }
    let error = toml::from_str::<AidConfig>(&document("").replace("agent = 'claude'", "agent = 'codex'"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("only agent = \"claude\" is supported"));
}
