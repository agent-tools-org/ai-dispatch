// Idle timeout helpers for task execution.
// Exports env-based readers/writers plus the shared 600s default.

use std::collections::HashMap;
use std::time::Duration;

pub(crate) const DEFAULT_IDLE_TIMEOUT_SECS: u64 = crate::timeout_policy::DEFAULT_IDLE_SECS;
pub(crate) const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS);
pub(crate) const IDLE_TIMEOUT_ENV: &str = crate::timeout_policy::ENV_IDLE_SECS;

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
    crate::timeout_policy::TimeoutPolicy::from_command(cmd).idle
}

pub(crate) fn idle_timeout_from_tokio_command(cmd: &tokio::process::Command) -> Duration {
    crate::timeout_policy::TimeoutPolicy::from_tokio_command(cmd).idle
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
    fn default_idle_timeout_is_600_seconds() {
        let cmd = std::process::Command::new("true");
        assert_eq!(idle_timeout_from_command(&cmd), DEFAULT_IDLE_TIMEOUT);
    }
}
