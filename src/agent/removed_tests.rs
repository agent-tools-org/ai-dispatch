// Removed-route validation at execution and configuration boundaries.
// Deps: isolated aid home, RunArgs, config writers, batch parser.
use super::*;
use crate::cmd::run::RunArgs;
use crate::types::AgentKind;

#[test]
fn removed_names_are_not_builtin_agents() {
    for name in ["gemini", "qwen", "Gemini", "QWEN"] {
        assert_eq!(AgentKind::parse_str(name), None);
        let error = reject(name).expect_err("removed").to_string();
        assert!(
            error.contains("removed in this release; use `agy`"),
            "{error}"
        );
        assert!(
            error.contains("`opencode`/`kilo` or a custom agent"),
            "{error}"
        );
    }
    for name in ["agy", "opencode", "kilo", "my-google-agent"] {
        reject(name).expect("supported name");
    }
}

#[test]
fn run_rejects_removed_primary_review_and_cascade_routes() {
    let home = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    for name in ["gemini", "qwen"] {
        for args in [
            RunArgs {
                agent_name: name.into(),
                ..Default::default()
            },
            RunArgs {
                agent_name: "codex".into(),
                judge: Some(name.into()),
                ..Default::default()
            },
            RunArgs {
                agent_name: "codex".into(),
                peer_review: Some(name.into()),
                ..Default::default()
            },
            RunArgs {
                agent_name: "codex".into(),
                cascade: vec![name.into()],
                ..Default::default()
            },
        ] {
            assert!(
                validate_run(&args)
                    .expect_err("removed route")
                    .to_string()
                    .contains("removed in this release")
            );
        }
    }
}

#[test]
fn removed_agent_config_is_rejected_on_write_but_ignored_on_dispatch() {
    let home = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    for name in ["gemini", "qwen"] {
        assert!(crate::config::agent::save_agent_disabled(name, true).is_err());
        assert!(crate::config::agent::save_agent_default_model(name, Some("model")).is_err());
        assert!(crate::config::agent::save_agent_idle_timeout(name, Some(30)).is_err());
        std::fs::write(
            home.path().join("agent_config.toml"),
            format!("[{name}]\nmodel = 'model'\n"),
        )
        .expect("config");
        assert!(!crate::config::agent::load_agent_config().contains_key(name));
        validate_run(&RunArgs { agent_name: "codex".into(), ..Default::default() })
            .expect("stale config must not block a supported dispatch");
        assert!(reject(name).is_err(), "explicit removed requests still fail");
    }
}

#[test]
fn team_and_batch_reject_removed_routes() {
    let home = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    for name in ["gemini", "qwen"] {
        let team =
            format!("[team]\nid = 'test'\ndisplay_name = 'Test'\npreferred_agents = ['{name}']\n");
        assert!(
            crate::config::team::parse_team(&team)
                .expect_err("removed preference")
                .to_string()
                .contains("removed in this release")
        );
        for field in ["agent", "judge", "peer_review", "fallback"] {
            let primary = if field == "agent" {
                ""
            } else {
                "agent = 'codex'\n"
            };
            let batch = format!("[[tasks]]\n{primary}{field} = '{name}'\nprompt = 'Review code'\n");
            let path = home.path().join("batch.toml");
            std::fs::write(&path, batch).expect("batch");
            assert!(
                crate::batch::parse_batch_file(&path)
                    .expect_err("removed batch route")
                    .to_string()
                    .contains("removed in this release")
            );
        }
    }
}

#[test]
fn removed_routes_in_unused_defaults_and_team_overrides_are_rejected() {
    let home = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    for name in ["gemini", "qwen"] {
        for setting in [
            format!("default_agent = '{name}'"),
            format!("[team.overrides.{name}]\nresearch = 10"),
        ] {
            let team = format!(
                "[team]\nid = 'test'\ndisplay_name = 'Test'\npreferred_agents = []\n{setting}\n"
            );
            assert!(
                crate::config::team::parse_team(&team)
                    .expect_err("removed setting")
                    .to_string()
                    .contains("removed in this release")
            );
        }
        for field in ["agent", "judge", "peer_review", "fallback"] {
            let path = home.path().join("batch.toml");
            std::fs::write(&path, format!("[defaults]\n{field} = '{name}'\n[[tasks]]\nagent = 'codex'\njudge = 'codex'\npeer_review = 'codex'\nfallback = 'codex'\nprompt = 'Review code'\n")).expect("batch");
            assert!(
                crate::batch::parse_batch_file(&path)
                    .expect_err("removed default")
                    .to_string()
                    .contains("removed in this release")
            );
        }
    }
}

#[test]
fn dispatch_does_not_silently_ignore_a_team_with_removed_preferences() {
    let home = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    std::fs::create_dir(home.path().join("teams")).expect("teams");
    for name in ["gemini", "qwen"] {
        std::fs::write(
            home.path().join("teams/test.toml"),
            format!("[team]\nid = 'test'\ndisplay_name = 'Test'\npreferred_agents = ['{name}']\n"),
        )
        .expect("team");
        let args = RunArgs {
            agent_name: "codex".into(),
            team: Some("test".into()),
            ..Default::default()
        };
        assert!(
            validate_run(&args)
                .expect_err("removed preference")
                .to_string()
                .contains("removed in this release")
        );
    }
}
