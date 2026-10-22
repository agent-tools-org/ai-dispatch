// Idle timeout helpers for task execution.
// Exports env-based readers/writers plus the shared 600s default.

use std::collections::HashMap;
#[cfg(test)]
use std::time::Duration;

pub(crate) const DEFAULT_IDLE_TIMEOUT_SECS: u64 = crate::config::timeout_policy::DEFAULT_IDLE_SECS;
#[cfg(test)]
pub(crate) const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS);
pub(crate) const IDLE_TIMEOUT_ENV: &str = crate::config::timeout_policy::ENV_IDLE_SECS;

pub(crate) fn idle_timeout_secs_from_env(env: Option<&HashMap<String, String>>) -> Option<u64> {
    env.and_then(|env| env.get(IDLE_TIMEOUT_ENV))
        .and_then(|value| parse_idle_timeout_secs(value))
}

#[cfg(test)]
pub(crate) fn idle_timeout_from_tokio_command(cmd: &tokio::process::Command) -> Duration {
    crate::config::timeout_policy::TimeoutPolicy::from_tokio_command(cmd).idle
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

}
