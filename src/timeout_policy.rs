// Unified timeout policy for task dispatch and monitoring.
// Exports TimeoutPolicy resolution plus env serialization for worker specs.
// Deps: project config, agent config, std::time, and HashMap env storage.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::Duration;

use crate::project::ProjectConfig;

pub(crate) const DEFAULT_IDLE_SECS: u64 = 600;
pub(crate) const DEFAULT_FIRST_TOKEN_SECS: u64 = 180;
pub(crate) const DEFAULT_WARN_SECS: u64 = 180;
pub(crate) const DEFAULT_NUDGE_SECS: u64 = 300;
pub(crate) const DEFAULT_ESCALATE_SECS: u64 = 600;
pub(crate) const DEFAULT_MAX_DURATION_MINS: i64 = 60;
pub(crate) const DEFAULT_HARD_CAP_HOURS: i64 = 24;

pub(crate) const ENV_IDLE_SECS: &str = "AID_IDLE_TIMEOUT_SECS";
const ENV_FIRST_TOKEN_SECS: &str = "AID_FIRST_TOKEN_TIMEOUT_SECS";
const ENV_WARN_SECS: &str = "AID_IDLE_WARN_SECS";
const ENV_NUDGE_SECS: &str = "AID_IDLE_NUDGE_SECS";
const ENV_ESCALATE_SECS: &str = "AID_IDLE_ESCALATE_SECS";
const ENV_MAX_DURATION_MINS: &str = "AID_MAX_DURATION_MINS";
const ENV_MAX_DURATION_SECS: &str = "AID_MAX_DURATION_SECS";
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
    pub(crate) first_token: Duration,
    pub(crate) nudge_ladder: NudgeLadder,
    pub(crate) max_duration: Duration,
    pub(crate) hard_cap: Duration,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(DEFAULT_IDLE_SECS),
            first_token: Duration::from_secs(DEFAULT_FIRST_TOKEN_SECS),
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
        
        let configured_idle = first_u64([cli_idle_secs, agent_idle_secs, project_idle_secs]);
        let idle_secs = configured_idle.unwrap_or(DEFAULT_IDLE_SECS);
        
        let first_token_base = configured_idle
            .map(|secs| std::cmp::max(secs, DEFAULT_FIRST_TOKEN_SECS))
            .unwrap_or(DEFAULT_FIRST_TOKEN_SECS);
            
        let first_token_env = std::env::var(ENV_FIRST_TOKEN_SECS)
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0);

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
            first_token: Duration::from_secs(first_token_env.unwrap_or(first_token_base)),
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
            first_token: Duration::from_secs(
                env_u64(env, ENV_FIRST_TOKEN_SECS).unwrap_or(DEFAULT_FIRST_TOKEN_SECS),
            ),
            nudge_ladder: NudgeLadder {
                warn: Duration::from_secs(env_u64(env, ENV_WARN_SECS).unwrap_or(DEFAULT_WARN_SECS)),
                nudge: Duration::from_secs(env_u64(env, ENV_NUDGE_SECS).unwrap_or(DEFAULT_NUDGE_SECS)),
                escalate: Duration::from_secs(
                    env_u64(env, ENV_ESCALATE_SECS).unwrap_or(DEFAULT_ESCALATE_SECS),
                ),
            },
            max_duration: env_u64(env, ENV_MAX_DURATION_SECS)
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_secs(
                    env_i64(env, ENV_MAX_DURATION_MINS).unwrap_or(DEFAULT_MAX_DURATION_MINS) as u64 * 60,
                )),
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
    env.entry(ENV_IDLE_SECS.to_string())
        .or_insert_with(|| policy.idle.as_secs().to_string());
    env.entry(ENV_FIRST_TOKEN_SECS.to_string())
        .or_insert_with(|| policy.first_token.as_secs().to_string());
    env.entry(ENV_WARN_SECS.to_string())
        .or_insert_with(|| policy.nudge_ladder.warn.as_secs().to_string());
    env.entry(ENV_NUDGE_SECS.to_string())
        .or_insert_with(|| policy.nudge_ladder.nudge.as_secs().to_string());
    env.entry(ENV_ESCALATE_SECS.to_string())
        .or_insert_with(|| policy.nudge_ladder.escalate.as_secs().to_string());
    env.entry(ENV_MAX_DURATION_MINS.to_string())
        .or_insert_with(|| policy.max_duration_mins().to_string());
    env.insert(ENV_MAX_DURATION_SECS.to_string(), policy.max_duration.as_secs().to_string());
    env.entry(ENV_HARD_CAP_HOURS.to_string())
        .or_insert_with(|| policy.hard_cap_hours().to_string());
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
mod tests;
