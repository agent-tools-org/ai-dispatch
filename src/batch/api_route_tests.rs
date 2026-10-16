// Batch and fallback acceptance of configured API route ids.
// Exercises existing name validation with temporary account configuration.
use super::*;
use crate::support::paths::{self, AidHomeGuard};

#[test]
fn batch_and_fallback_names_accept_api_routes() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-meet'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    assert!(is_valid_agent("claude-api-meet"));
    assert!(!is_valid_agent("claude-api-unknown"));
    let task: BatchTask =
        toml::from_str("prompt = 'test'\nagent = 'claude-api-meet'\nfallback = 'claude-api-meet,claude'")
            .unwrap();
    validate_agents(std::slice::from_ref(&task)).unwrap();
    validate_fallback_agents(&[task]).unwrap();
}
