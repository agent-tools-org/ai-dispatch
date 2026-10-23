// Regression coverage for route identity after held-agent cascade substitution.
// Uses isolated Codex configuration, markers and an in-memory store.
use super::*;
use crate::support::paths::AidHomeGuard;

struct ConfigGuard;
impl Drop for ConfigGuard {
    fn drop(&mut self) {
        agent::codex::cli_config::set_test_codex_home(None);
    }
}

fn select(home: &std::path::Path, config: &str) -> ConfigGuard {
    std::fs::write(home.join("config.toml"), config).unwrap();
    agent::codex::cli_config::set_test_codex_home(Some(home.to_path_buf()));
    ConfigGuard
}

fn hold(kind: AgentKind, name: Option<&str>) {
    std::fs::write(
        rate_limit::marker_path(&kind, name),
        "recovery_at: \nhold: manual\nmessage: test quota exhausted\n",
    )
    .unwrap();
}

fn args(name: &str, cascade: &[&str]) -> RunArgs {
    RunArgs {
        agent_name: name.into(),
        prompt: "Explain routing".into(),
        cascade: cascade.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn held_opencode_cascade_recomputes_final_codex_provider_identity() {
    for (config, expected) in [("", None), ("model_provider='X'", Some("codex-provider:X"))] {
        let home = tempfile::tempdir().unwrap();
        let _aid = AidHomeGuard::set(home.path());
        let _codex = select(home.path(), config);
        hold(AgentKind::OpenCode, None);
        let store = Arc::new(Store::open_memory().unwrap());
        let setup = resolve_agent_setup(&store, &mut args("opencode", &["codex"]), None).unwrap();
        assert_eq!(setup.agent_kind, AgentKind::Codex);
        assert_eq!(setup.agent_display_name, "codex");
        assert_eq!(setup.custom_agent_name.as_deref(), expected);
        assert_eq!(setup.agent.rate_limit_name(), expected);
    }
}

#[test]
fn held_codex_provider_is_skipped_and_exhausted_cascade_names_its_clear_command() {
    let home = tempfile::tempdir().unwrap();
    let _aid = AidHomeGuard::set(home.path());
    let _codex = select(home.path(), "model_provider='X'");
    hold(AgentKind::Codex, Some("codex-provider:X"));
    let store = Arc::new(Store::open_memory().unwrap());
    let builds = agent::codex::command_builds::count();
    let setup =
        resolve_agent_setup(&store, &mut args("codex", &["codex", "opencode"]), None).unwrap();
    assert_eq!(setup.agent_kind, AgentKind::OpenCode);
    assert_eq!(setup.custom_agent_name, None);
    let result = resolve_agent_setup(&store, &mut args("codex", &["codex"]), None);
    let error = result
        .err()
        .expect("held route cannot construct a launch adapter");
    assert!(error.to_string().contains("clear-limit codex-provider:X"));
    assert_eq!(agent::codex::command_builds::count(), builds);
    let error =
        super::super::run_dispatch_prepare::prepare_dispatch_with(&store, &mut args("codex", &[]), |_| true)
            .err()
            .expect("held provider must refuse before task persistence");
    assert!(error.to_string().contains("clear-limit codex-provider:X"));
    assert!(store.list_tasks(crate::types::TaskFilter::All).unwrap().is_empty());
    assert_eq!(agent::codex::command_builds::count(), builds);
}

#[test]
fn default_codex_keeps_none_and_its_existing_hold_hint() {
    let home = tempfile::tempdir().unwrap();
    let _aid = AidHomeGuard::set(home.path());
    let _codex = select(home.path(), "model_provider='openai'");
    let store = Arc::new(Store::open_memory().unwrap());
    let setup = resolve_agent_setup(&store, &mut args("codex", &[]), None).unwrap();
    assert_eq!(setup.custom_agent_name, None);
    assert_eq!(setup.agent_display_name, "codex");
    assert_eq!(setup.agent.rate_limit_name(), None);
    hold(AgentKind::Codex, None);
    let error = resolve_agent_setup(&store, &mut args("codex", &[]), None)
        .err()
        .unwrap();
    assert!(error.to_string().contains("clear-limit codex`"));
    std::fs::remove_file(rate_limit::marker_path(&AgentKind::Codex, None)).unwrap();
    hold(AgentKind::Codex, Some("codex-provider:X"));
    assert!(resolve_agent_setup(&store, &mut args("codex", &[]), None).is_ok());
}

#[test]
fn background_provider_hold_ignores_healthy_plan_and_substitutes_or_refuses() {
    let home = tempfile::tempdir().unwrap();
    let _aid = AidHomeGuard::set(home.path());
    let _codex = select(home.path(), "model_provider='X'");
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    std::fs::write(
        home.path().join("codex.json"),
        serde_json::json!({"ok":true,
            "snapshot":{"provider":"codex","fetched_at":"2099-01-01T00:00:00Z",
            "windows":[{"label":"5h","used_percent":1,"resets_at":"2099-01-02T00:00:00Z"}]}
        })
        .to_string(),
    )
    .unwrap();
    let route = Some("codex-provider:X");
    rate_limit::mark_rate_limited(&AgentKind::Codex, route, "You have hit your usage limit");
    let info = rate_limit::get_rate_limit_info(&AgentKind::Codex, route).unwrap();
    assert!(info.needs_human);
    assert!(info.recovery_at.is_none(), "a dateless provider refusal has no reset clock");
    let store = Arc::new(Store::open_memory().unwrap());
    let builds = agent::codex::command_builds::count();
    for cascade in [vec!["opencode"], vec![]] {
        let mut request = args("codex", &cascade);
        request.declared_urgency = Some(crate::types::TaskUrgency::Background);
        let result = resolve_agent_setup(&store, &mut request, None);
        if cascade.is_empty() {
            assert!(
                result
                    .err()
                    .expect("held provider must refuse")
                    .to_string()
                    .contains("clear-limit codex-provider:X")
            );
        } else {
            assert_eq!(result.unwrap().agent_kind, AgentKind::OpenCode);
        }
    }
    assert_eq!(agent::codex::command_builds::count(), builds);
}
