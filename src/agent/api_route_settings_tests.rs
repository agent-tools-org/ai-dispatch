// Managed settings are presence-based launch blockers, without reading their contents.
// The Library root is replaced only through the private test seam.
use super::*;

struct LibraryOverride;
impl Drop for LibraryOverride {
    fn drop(&mut self) {
        TEST_LIBRARY_ROOT.with(|root| root.replace(None));
    }
}

fn opts(home: &std::path::Path) -> RunOpts {
    RunOpts {
        dir: Some(home.display().to_string()),
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: Some(std::collections::HashMap::from([(
            "HOME".into(),
            home.display().to_string(),
        )])),
        env_forward: None,
    }
}

#[test]
fn managed_api_key_helper_override_refuses_route_launch() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    let library = home.path().join("Library");
    TEST_LIBRARY_ROOT.with(|root| root.replace(Some(library.clone())));
    let _reset = LibraryOverride;
    let route: ApiRoute = toml::from_str(concat!(
        "id = 'claude-api-ming'\nagent = 'claude'\nkeychain_service = 'test'\nmonthly_budget_usd = 200\n"
    ))
    .unwrap();
    let agent = super::super::ApiRouteAgent(route);
    let opts = opts(home.path());
    for relative in [
        "Application Support/ClaudeCode/managed-settings.json",
        "Application Support/ClaudeCode/managed-settings.d",
        "Managed Preferences/com.anthropic.claudecode.plist",
        "Managed Preferences/another-user/com.anthropic.claudecode.plist",
    ] {
        let path = library.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if relative.ends_with(".d") {
            std::fs::create_dir(&path).unwrap();
        } else {
            std::fs::write(&path, r#"{"apiKeyHelper":"different credential"}"#).unwrap();
        }
        let error = super::super::Agent::build_command(&agent, "test", &opts)
            .unwrap_err()
            .to_string();
        assert!(error.contains("managed settings source exists"));
        assert!(error.contains(path.to_str().unwrap()));
        assert!(!error.contains("different credential"));
        assert!(super::super::Agent::build_command(&super::super::ClaudeAgent, "test", &opts).is_ok());
        if path.is_dir() {
            std::fs::remove_dir(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
    }
    assert!(super::super::Agent::build_command(&agent, "test", &opts).is_ok());
}
