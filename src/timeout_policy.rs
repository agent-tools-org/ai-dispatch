// Unified timeout policy for task dispatch and monitoring.
// Exports TimeoutPolicy resolution plus env serialization for worker specs.
// Deps: project config, agent config, std::time, and HashMap env storage.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::Duration;

use crate::project::ProjectConfig;

pub(crate) const DEFAULT_IDLE_SECS: u64 = 600;
pub(crate) const DEFAULT_WARN_SECS: u64 = 180;
pub(crate) const DEFAULT_NUDGE_SECS: u64 = 300;
pub(crate) const DEFAULT_ESCALATE_SECS: u64 = 600;
pub(crate) const DEFAULT_MAX_DURATION_MINS: i64 = 60;
pub(crate) const DEFAULT_HARD_CAP_HOURS: i64 = 24;

pub(crate) const ENV_IDLE_SECS: &str = "AID_IDLE_TIMEOUT_SECS";
const ENV_WARN_SECS: &str = "AID_IDLE_WARN_SECS";
const ENV_NUDGE_SECS: &str = "AID_IDLE_NUDGE_SECS";
const ENV_ESCALATE_SECS: &str = "AID_IDLE_ESCALATE_SECS";
const ENV_MAX_DURATION_MINS: &str = "AID_MAX_DURATION_MINS";
const ENV_HARD_CAP_HOURS: &str = "AID_HARD_CAP_HOURS";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NudgeLadder {
    pub(crate) warn: Duration,
    pub(crate) nudge: Duration,
    pub(crate) escalate: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TimeoutPolicy {
    pub(crate) idle: Duration,
    pub(crate) nudge_ladder: NudgeLadder,
    pub(crate) max_duration: Duration,
    pub(crate) hard_cap: Duration,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(DEFAULT_IDLE_SECS),
            nudge_ladder: NudgeLadder {
                warn: Duration::from_secs(DEFAULT_WARN_SECS),
                nudge: Duration::from_secs(DEFAULT_NUDGE_SECS),
                escalate: Duration::from_secs(DEFAULT_ESCALATE_SECS),
            },
            max_duration: Duration::from_secs(DEFAULT_MAX_DURATION_MINS as u64 * 60),
            hard_cap: Duration::from_secs(DEFAULT_HARD_CAP_HOURS as u64 * 60 * 60),
        }
    }
}

impl TimeoutPolicy {
    pub(crate) fn resolve(
        agent_name: &str,
        cli_idle_secs: Option<u64>,
        cli_max_duration_mins: Option<i64>,
        project: Option<&ProjectConfig>,
    ) -> Self {
        let defaults = Self::default();
        let agent_idle_secs = crate::agent_config::get_default_idle_timeout(agent_name);
        let project_idle_secs = project.and_then(|project| project.idle_timeout);
        let idle_secs = first_u64([cli_idle_secs, agent_idle_secs, project_idle_secs])
            .unwrap_or(DEFAULT_IDLE_SECS);
        let max_duration_mins = first_i64([
            cli_max_duration_mins,
            project.and_then(|project| project.max_duration_mins),
        ])
        .unwrap_or(DEFAULT_MAX_DURATION_MINS);
        let hard_cap_hours = first_i64([project.and_then(|project| project.hard_cap_hours)])
            .unwrap_or(DEFAULT_HARD_CAP_HOURS);
        let ladder = project.map_or(defaults.nudge_ladder, project_ladder);
        Self {
            idle: Duration::from_secs(idle_secs),
            nudge_ladder: ladder,
            max_duration: Duration::from_secs(max_duration_mins as u64 * 60),
            hard_cap: Duration::from_secs(hard_cap_hours as u64 * 60 * 60),
        }
    }

    pub(crate) fn from_env(env: Option<&HashMap<String, String>>) -> Self {
        let defaults = Self::default();
        let Some(env) = env else {
            return defaults;
        };
        Self {
            idle: Duration::from_secs(env_u64(env, ENV_IDLE_SECS).unwrap_or(DEFAULT_IDLE_SECS)),
            nudge_ladder: NudgeLadder {
                warn: Duration::from_secs(env_u64(env, ENV_WARN_SECS).unwrap_or(DEFAULT_WARN_SECS)),
                nudge: Duration::from_secs(env_u64(env, ENV_NUDGE_SECS).unwrap_or(DEFAULT_NUDGE_SECS)),
                escalate: Duration::from_secs(
                    env_u64(env, ENV_ESCALATE_SECS).unwrap_or(DEFAULT_ESCALATE_SECS),
                ),
            },
            max_duration: Duration::from_secs(
                env_i64(env, ENV_MAX_DURATION_MINS).unwrap_or(DEFAULT_MAX_DURATION_MINS) as u64 * 60,
            ),
            hard_cap: Duration::from_secs(
                env_i64(env, ENV_HARD_CAP_HOURS).unwrap_or(DEFAULT_HARD_CAP_HOURS) as u64 * 60 * 60,
            ),
        }
    }

    pub(crate) fn from_command(cmd: &std::process::Command) -> Self {
        Self::from_env_pairs(cmd.get_envs())
    }

    pub(crate) fn from_tokio_command(cmd: &tokio::process::Command) -> Self {
        Self::from_env_pairs(cmd.as_std().get_envs())
    }

    pub(crate) fn max_duration_mins(self) -> i64 {
        (self.max_duration.as_secs() / 60) as i64
    }

    pub(crate) fn hard_cap_hours(self) -> i64 {
        (self.hard_cap.as_secs() / 60 / 60) as i64
    }

    fn from_env_pairs<'a, I>(envs: I) -> Self
    where
        I: Iterator<Item = (&'a OsStr, Option<&'a OsStr>)>,
    {
        let env = envs
            .filter_map(|(key, value)| {
                Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
            })
            .collect::<HashMap<_, _>>();
        Self::from_env(Some(&env))
    }
}

pub(crate) fn env_with_policy(
    env: Option<HashMap<String, String>>,
    policy: TimeoutPolicy,
) -> Option<HashMap<String, String>> {
    let mut env = env.unwrap_or_default();
    env.insert(ENV_IDLE_SECS.to_string(), policy.idle.as_secs().to_string());
    env.insert(
        ENV_WARN_SECS.to_string(),
        policy.nudge_ladder.warn.as_secs().to_string(),
    );
    env.insert(
        ENV_NUDGE_SECS.to_string(),
        policy.nudge_ladder.nudge.as_secs().to_string(),
    );
    env.insert(
        ENV_ESCALATE_SECS.to_string(),
        policy.nudge_ladder.escalate.as_secs().to_string(),
    );
    env.insert(
        ENV_MAX_DURATION_MINS.to_string(),
        policy.max_duration_mins().to_string(),
    );
    env.insert(ENV_HARD_CAP_HOURS.to_string(), policy.hard_cap_hours().to_string());
    Some(env)
}

fn project_ladder(project: &ProjectConfig) -> NudgeLadder {
    NudgeLadder {
        warn: Duration::from_secs(
            first_u64([project.idle_warn_secs, project.unstick.warn_after_secs])
                .unwrap_or(DEFAULT_WARN_SECS),
        ),
        nudge: Duration::from_secs(
            first_u64([project.idle_nudge_secs, project.unstick.nudge_after_secs])
                .unwrap_or(DEFAULT_NUDGE_SECS),
        ),
        escalate: Duration::from_secs(
            first_u64([project.idle_escalate_secs, project.unstick.escalate_after_secs])
                .unwrap_or(DEFAULT_ESCALATE_SECS),
        ),
    }
}

fn first_u64<const N: usize>(values: [Option<u64>; N]) -> Option<u64> {
    values.into_iter().flatten().find(|value| *value > 0)
}

fn first_i64<const N: usize>(values: [Option<i64>; N]) -> Option<i64> {
    values.into_iter().flatten().find(|value| *value > 0)
}

fn env_u64(env: &HashMap<String, String>, key: &str) -> Option<u64> {
    env.get(key).and_then(|value| value.parse().ok()).filter(|value| *value > 0)
}

fn env_i64(env: &HashMap<String, String>, key: &str) -> Option<i64> {
    env.get(key).and_then(|value| value.parse().ok()).filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectConfig, ProjectUnstickConfig};
    use crate::paths::AidHomeGuard;
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
        let policy = TimeoutPolicy::resolve("project-only-agent", None, None, Some(&project));
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
        assert_eq!(TimeoutPolicy::from_tokio_command(&tokio_cmd).idle, policy.idle);
        assert_eq!(crate::idle_timeout::idle_timeout_from_tokio_command(&tokio_cmd), policy.idle);
    }
}
