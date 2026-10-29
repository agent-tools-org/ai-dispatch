// Per-CLI variables that point an agent's config/state away from `$HOME`.
// Exports: CONFIG_REDIRECT_VARS, strip_inherited_config_redirects.
// Deps: std::process::Command. An inherited one would bypass the isolated HOME.

use std::process::Command;

/// Each variable makes its CLI read or write config, credentials, instructions
/// or state somewhere other than under `$HOME`, so an operator value inherited
/// by a task reaches the real files the isolated HOME exists to mask. A task
/// that needs one names it with `--env` or `--env-forward`, which apply after.
pub const CONFIG_REDIRECT_VARS: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    "CODEX_HOME",
    "CODEX_SQLITE_HOME",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_CONFIG",
    "OPENCODE_DB",
    "COPILOT_HOME",
    "CURSOR_CONFIG_DIR",
    "CURSOR_DATA_DIR",
    "GROK_HOME",
    "FACTORY_HOME_OVERRIDE",
    "MIMOCODE_HOME",
];

/// Remove redirect variables except Codex's validated provider credential references.
/// Durable Codex home and explicit task env are applied after this and still take effect.
pub fn strip_inherited_config_redirects(cmd: &mut Command) {
    let provider_env = super::codex::provider::referenced_env(cmd);
    for name in CONFIG_REDIRECT_VARS {
        if !provider_env.iter().any(|provider_name| provider_name.as_str() == *name) {
            cmd.env_remove(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn removed_agent_redirects_are_not_managed() {
        assert!(!CONFIG_REDIRECT_VARS.contains(&"GEMINI_CLI_HOME"));
    }

    #[test]
    fn redirect_vars_are_unique() {
        let unique: HashSet<_> = CONFIG_REDIRECT_VARS.iter().collect();
        assert_eq!(unique.len(), CONFIG_REDIRECT_VARS.len());
    }

    #[test]
    fn strip_marks_every_redirect_var_removed() {
        let mut cmd = Command::new("true");
        cmd.env("CLAUDE_CONFIG_DIR", "/operator/claude");
        strip_inherited_config_redirects(&mut cmd);
        for name in CONFIG_REDIRECT_VARS {
            let entry = cmd.get_envs().find(|(key, _)| key == name);
            assert_eq!(entry.map(|(_, value)| value), Some(None), "{name} not removed");
        }
    }

    #[test]
    fn run_env_forward_overrides_explicit_value_at_launch() {
        let _aid_home = crate::support::test_env::TestHome::new();
        let opts = crate::agent::RunOpts {
            env: Some(std::collections::HashMap::from([("PATH".to_string(), "explicit".to_string())])),
            env_forward: Some(vec!["PATH".to_string()]),
            ..Default::default()
        };
        let guard = crate::agent::home_isolation::IsolatedHomeGuard::create(None).unwrap();
        let mut cmd = Command::new("true");
        crate::agent::apply_run_env(&mut cmd, &opts, &guard);
        assert_eq!(launched(&cmd, "PATH"), Some(std::env::var("PATH").ok()));
    }

    fn launch_env(env: Option<std::collections::HashMap<String, String>>) -> Command {
        let _aid_home = crate::support::test_env::TestHome::new();
        let opts = crate::agent::RunOpts {
            env,
            ..Default::default()
        };
        let guard = crate::agent::home_isolation::IsolatedHomeGuard::create(None).unwrap();
        let mut cmd = Command::new("true");
        crate::agent::apply_run_env(&mut cmd, &opts, &guard);
        cmd
    }

    fn launched(cmd: &Command, name: &str) -> Option<Option<String>> {
        cmd.get_envs()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn run_env_drops_inherited_claude_config_dir() {
        let cmd = launch_env(None);
        assert_eq!(launched(&cmd, "CLAUDE_CONFIG_DIR"), Some(None));
        assert_eq!(launched(&cmd, "OPENCODE_CONFIG_DIR"), Some(None));
    }

    #[test]
    fn run_env_keeps_explicit_task_value() {
        let env = std::collections::HashMap::from([(
            "CLAUDE_CONFIG_DIR".to_string(),
            "/task/claude".to_string(),
        )]);
        let cmd = launch_env(Some(env));
        assert_eq!(launched(&cmd, "CLAUDE_CONFIG_DIR"), Some(Some("/task/claude".to_string())));
    }
}
