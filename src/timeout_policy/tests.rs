// Tests for resolved timeout policy values.
// Covers precedence, env serialization, and command env extraction.
// Deps: timeout_policy internals, project config, and command builders.

use super::*;
use crate::paths::AidHomeGuard;
use crate::project::{ProjectConfig, ProjectUnstickConfig};
use std::fs;

#[test]
fn policy_resolution_precedence_is_cli_agent_project_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::agent_config::save_agent_idle_timeout("codex", Some(420)).expect("save config");
    let project = ProjectConfig {
        idle_timeout: Some(300),
        max_duration_mins: Some(20),
        ..Default::default()
    };

    let policy = TimeoutPolicy::resolve("codex", Some(240), Some(10), Some(&project));
    assert_eq!(policy.idle, Duration::from_secs(240));
    assert_eq!(policy.max_duration, Duration::from_secs(10 * 60));

    let policy = TimeoutPolicy::resolve("codex", None, None, Some(&project));
    assert_eq!(policy.idle, Duration::from_secs(420));
    assert_eq!(policy.max_duration, Duration::from_secs(20 * 60));

    let policy = TimeoutPolicy::resolve("project-only-agent", None, None, Some(&project));
    assert_eq!(policy.idle, Duration::from_secs(300));

    let policy = TimeoutPolicy::resolve("project-only-agent", None, None, None);
    assert_eq!(policy.idle, Duration::from_secs(DEFAULT_IDLE_SECS));
    assert_eq!(
        policy.max_duration,
        Duration::from_secs(DEFAULT_MAX_DURATION_MINS as u64 * 60)
    );
}

#[test]
fn policy_env_round_trips_all_fields() {
    let project = ProjectConfig {
        idle_timeout: Some(222),
        max_duration_mins: Some(33),
        hard_cap_hours: Some(7),
        unstick: ProjectUnstickConfig {
            warn_after_secs: Some(11),
            nudge_after_secs: Some(22),
            escalate_after_secs: Some(44),
        },
        ..Default::default()
    };
    let mut policy = TimeoutPolicy::resolve("project-only-agent", None, None, Some(&project));
    policy.first_token = Duration::from_secs(123);
    let env = env_with_policy(None, policy);

    assert_eq!(TimeoutPolicy::from_env(env.as_ref()), policy);
}

#[test]
fn project_toml_timeout_fields_parse() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("project.toml");
    fs::write(
        &path,
        r#"[project]
id = "demo"
idle_timeout = 222
max_duration_mins = 33
hard_cap_hours = 7

[project.unstick]
warn_after_secs = 11
nudge_after_secs = 22
escalate_after_secs = 44
"#,
    )
    .expect("write project");

    let project = crate::project::load_project(&path).expect("load project");
    let policy = TimeoutPolicy::resolve("project-only-agent", None, None, Some(&project));

    assert_eq!(policy.idle, Duration::from_secs(222));
    assert_eq!(policy.max_duration, Duration::from_secs(33 * 60));
    assert_eq!(policy.hard_cap, Duration::from_secs(7 * 60 * 60));
    assert_eq!(policy.nudge_ladder.warn, Duration::from_secs(11));
}

#[test]
fn std_and_tokio_command_pipelines_read_same_idle_value() {
    let policy = TimeoutPolicy {
        idle: Duration::from_secs(123),
        first_token: Duration::from_secs(45),
        ..TimeoutPolicy::default()
    };
    let env = env_with_policy(None, policy).expect("policy env");
    let mut std_cmd = std::process::Command::new("true");
    let mut tokio_std_cmd = std::process::Command::new("true");
    for (key, value) in env {
        std_cmd.env(&key, &value);
        tokio_std_cmd.env(key, value);
    }
    let tokio_cmd = tokio::process::Command::from(tokio_std_cmd);

    assert_eq!(TimeoutPolicy::from_command(&std_cmd).idle, policy.idle);
    assert_eq!(TimeoutPolicy::from_command(&std_cmd).first_token, policy.first_token);
    assert_eq!(TimeoutPolicy::from_tokio_command(&tokio_cmd).idle, policy.idle);
    assert_eq!(
        TimeoutPolicy::from_tokio_command(&tokio_cmd).first_token,
        policy.first_token
    );
    assert_eq!(crate::idle_timeout::idle_timeout_from_tokio_command(&tokio_cmd), policy.idle);
}
