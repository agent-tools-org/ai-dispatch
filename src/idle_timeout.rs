// Idle timeout helpers for task execution.
// Exports env-based readers/writers plus the shared 300s default.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::Duration;

pub(crate) const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS);
pub(crate) const IDLE_TIMEOUT_ENV: &str = "AID_IDLE_TIMEOUT_SECS";

pub(crate) fn env_with_idle_timeout(
    env: Option<HashMap<String, String>>,
    idle_timeout_secs: Option<u64>,
) -> Option<HashMap<String, String>> {
    let Some(idle_timeout_secs) = idle_timeout_secs.filter(|secs| *secs > 0) else {
        return env;
    };
    let mut env = env.unwrap_or_default();
    env.insert(IDLE_TIMEOUT_ENV.to_string(), idle_timeout_secs.to_string());
    Some(env)
}

pub(crate) fn idle_timeout_secs_from_env(env: Option<&HashMap<String, String>>) -> Option<u64> {
    env.and_then(|env| env.get(IDLE_TIMEOUT_ENV))
        .and_then(|value| parse_idle_timeout_secs(value))
}

pub(crate) fn idle_timeout_from_command(cmd: &std::process::Command) -> Duration {
    idle_timeout_from_envs(cmd.get_envs())
}

pub(crate) fn idle_timeout_from_tokio_command(cmd: &tokio::process::Command) -> Duration {
    idle_timeout_from_envs(cmd.as_std().get_envs())
}

fn idle_timeout_from_envs<'a, I>(mut envs: I) -> Duration
where
    I: Iterator<Item = (&'a OsStr, Option<&'a OsStr>)>,
{
    envs.find_map(|(key, value)| {
        if key != OsStr::new(IDLE_TIMEOUT_ENV) {
            return None;
        }
        value.and_then(OsStr::to_str).and_then(parse_idle_timeout_secs)
    })
    .map(Duration::from_secs)
    .unwrap_or(DEFAULT_IDLE_TIMEOUT)
}

fn parse_idle_timeout_secs(value: &str) -> Option<u64> {
    value.parse().ok().filter(|secs| *secs > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::parse_batch_file;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn batch_idle_timeout_parses_from_toml() {
        let file = write_temp(concat!(
            "[defaults]\nagent = \"codex\"\nidle_timeout = 300\n",
            "[[tasks]]\nprompt = \"test\"\n"
        ));
        let config = parse_batch_file(file.path()).unwrap();

        assert_eq!(config.defaults.idle_timeout, Some(300));
        assert_eq!(config.tasks[0].idle_timeout, Some(300));
    }

    #[test]
    fn default_idle_timeout_is_300_seconds() {
        let cmd = std::process::Command::new("true");
        assert_eq!(idle_timeout_from_command(&cmd), DEFAULT_IDLE_TIMEOUT);
    }
}
