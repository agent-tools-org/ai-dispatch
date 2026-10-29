// API-route commands must retain isolated settings and scrubbed key environment after prepare.
// Tests use synthetic route metadata and never invoke the keychain or a model.
use super::*;
use crate::agent::{
    Agent, RunOpts,
    api_route::{ApiRouteAgent, ROUTE_ENV_REMOVALS},
};

#[test]
fn prepare_preserves_api_route_settings_and_scrubbed_environment() {
    let home = crate::support::test_env::TestHome::new();
    let route = ApiRouteAgent(
        serde_json::from_value(json!({
            "id":"synthetic-route", "agent":"claude", "keychain_service":"synthetic",
            "monthly_budget_usd":100,
        }))
        .unwrap(),
    );
    let opts = opts(home.path());
    let mut command: tokio::process::Command =
        route.build_command("initial prompt", &opts).unwrap().into();
    let before = args(&command);
    assert!(prepare(&route, &mut command).unwrap().is_some());
    let after = args(&command);
    for flag in ["--setting-sources", "--settings"] {
        let expected = before.windows(2).find(|pair| pair[0] == flag).unwrap();
        assert!(after.windows(2).any(|pair| pair == expected));
    }
    assert!(
        after
            .windows(2)
            .any(|pair| pair[0] == "--setting-sources" && pair[1].is_empty())
    );
    let inline = after
        .windows(2)
        .find(|pair| pair[0] == "--settings")
        .unwrap();
    let settings: Value = serde_json::from_str(inline[1].to_str().unwrap()).unwrap();
    assert_eq!(
        settings["apiKeyHelper"],
        "/usr/bin/security find-generic-password -s synthetic -a synthetic-route -w"
    );
    for name in ROUTE_ENV_REMOVALS {
        assert!(
            command
                .as_std()
                .get_envs()
                .any(|(key, value)| key == *name && value.is_none())
        );
    }
    assert!(supports(&route));
    assert!(supports(&crate::agent::claude::ClaudeAgent));
    assert!(!supports(&crate::runtime::watcher::idle_tests::FakeAgent));
}

fn opts(home: &std::path::Path) -> RunOpts {
    RunOpts {
        dir: Some(home.display().to_string()),
        model: Some("sonnet".into()),
        env: Some(std::collections::HashMap::from([
            ("HOME".into(), home.display().to_string()),
            ("ANTHROPIC_API_KEY".into(), "synthetic".into()),
        ])),
        ..Default::default()
    }
}

fn args(command: &tokio::process::Command) -> Vec<std::ffi::OsString> {
    command
        .as_std()
        .get_args()
        .map(|arg| arg.to_owned())
        .collect()
}
