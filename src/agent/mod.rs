// Agent trait and registry for AI CLI adapters.
// Each agent knows how to build its CLI command and parse its output.

pub mod codebuff;
pub mod codex;
pub mod cursor;
pub mod droid;
pub mod gemini;
pub mod kilo;
pub mod opencode;
pub mod oz;
pub(crate) mod custom;
pub(crate) mod registry;
pub mod classifier;
pub(crate) mod selection;
pub(crate) mod truncate;

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::store;
use crate::types::*;

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const CARGO_MANIFEST_NAME: &str = "Cargo.toml";
const SHARED_TARGET_DIR_NAME: &str = "cargo-target";

/// Adapter trait for AI CLI tools
pub trait Agent: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Whether this agent streams JSONL (true) or outputs a single JSON blob (false)
    fn streaming(&self) -> bool;

    /// Build the OS command to execute this agent
    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command>;

    /// Parse a single line of output into an event (streaming agents only)
    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent>;

    /// Parse buffered output into completion info (non-streaming agents)
    fn parse_completion(&self, output: &str) -> CompletionInfo;

    /// Whether this agent requires a PTY even for foreground execution.
    /// Agents that don't produce stdout when piped (e.g. opencode) should return true.
    fn needs_pty(&self) -> bool {
        false
    }
}

/// Options passed to agent for command construction
#[derive(Debug, Clone)]
pub struct RunOpts {
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub budget: bool,
    pub read_only: bool,
    pub context_files: Vec<String>,
    pub session_id: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
}

/// Detect which agents are installed on the system
pub fn detect_agents() -> Vec<AgentKind> {
    let mut found = Vec::new();
    for (name, kind) in [
        ("gemini", AgentKind::Gemini),
        ("codex", AgentKind::Codex),
        ("opencode", AgentKind::OpenCode),
        ("cursor-agent", AgentKind::Cursor),
        ("droid", AgentKind::Droid),
        ("kilo", AgentKind::Kilo),
        ("aid-codebuff", AgentKind::Codebuff),
        ("oz", AgentKind::Oz),
    ] {
        if which_exists(name) {
            found.push(kind);
        }
    }
    found
}

pub(crate) fn select_agent_with_reason(
    prompt: &str, opts: &RunOpts, store: &store::Store,
    team: Option<&crate::team::TeamConfig>,
) -> (String, String) {
    selection::select_agent_with_reason(prompt, opts, store, team)
}

/// Get an agent adapter by kind
pub fn get_agent(kind: AgentKind) -> Box<dyn Agent> {
    match kind {
        AgentKind::Codex => Box::new(codex::CodexAgent),
        AgentKind::Cursor => Box::new(cursor::CursorAgent),
        AgentKind::Gemini => Box::new(gemini::GeminiAgent),
        AgentKind::OpenCode => Box::new(opencode::OpenCodeAgent),
        AgentKind::Kilo => Box::new(kilo::KiloAgent),
        AgentKind::Codebuff => Box::new(codebuff::CodebuffAgent),
        AgentKind::Droid => Box::new(droid::DroidAgent),
        AgentKind::Oz => Box::new(oz::OzAgent),
        AgentKind::Custom => panic!("Custom agents must be resolved via resolve_agent()"),
    }
}

pub fn agent_has_fs_access(_kind: &AgentKind) -> bool {
    true // all supported agents have file system access
}

pub fn shared_target_dir() -> Option<String> {
    if let Some(target_dir) = std::env::var_os(CARGO_TARGET_DIR_ENV) {
        return Some(target_dir.to_string_lossy().into_owned());
    }

    Some(
        crate::paths::aid_dir()
            .join(SHARED_TARGET_DIR_NAME)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Returns a target directory isolated per worktree branch.
/// Worktree tasks get `{base}/{sanitized_branch}` to avoid lock contention.
/// Non-worktree tasks share the base directory.
pub fn target_dir_for_worktree(worktree_branch: Option<&str>) -> Option<String> {
    let base = shared_target_dir()?;
    match worktree_branch {
        Some(branch) => {
            let sanitized = branch.replace('/', "-");
            Some(format!("{base}/{sanitized}"))
        }
        None => Some(base),
    }
}

pub fn is_rust_project(dir: Option<&str>) -> bool {
    resolve_project_dir(dir).join(CARGO_MANIFEST_NAME).is_file()
}

fn resolve_project_dir(dir: Option<&str>) -> PathBuf {
    dir.map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Set GIT_CEILING_DIRECTORIES on a command to prevent git from ascending
/// above the target --dir. This stops agents from discovering and modifying
/// the host git repo when --dir points to a non-repo directory.
pub fn set_git_ceiling(cmd: &mut Command, dir: &str) {
    let path = std::path::Path::new(dir);
    if let Some(parent) = path.parent() {
        cmd.env("GIT_CEILING_DIRECTORIES", parent);
    }
}

pub fn apply_run_env(cmd: &mut Command, opts: &RunOpts) {
    if let Some(env) = opts.env.as_ref() {
        for (key, value) in env {
            cmd.env(key, value);
        }
    }
    if let Some(env_forward) = opts.env_forward.as_ref() {
        for name in env_forward {
            if let Ok(value) = std::env::var(name) {
                cmd.env(name, value);
            }
        }
    }
}

fn which_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{apply_run_env, is_rust_project, set_git_ceiling, shared_target_dir, target_dir_for_worktree, RunOpts};
    use crate::test_subprocess;
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn set_git_ceiling_uses_parent_dir() {
        let mut cmd = Command::new("echo");
        set_git_ceiling(&mut cmd, "/tmp/cloned-repo");
        let envs: Vec<_> = cmd.get_envs().collect();
        let ceiling = envs
            .iter()
            .find(|(k, _)| *k == "GIT_CEILING_DIRECTORIES")
            .and_then(|(_, v)| v.as_ref())
            .map(|v| v.to_string_lossy().to_string());
        assert_eq!(ceiling.as_deref(), Some("/tmp"));
    }

    #[test]
    fn detects_rust_project_in_current_dir() {
        let _permit = test_subprocess::acquire();
        let temp_dir = TempDir::new().unwrap();

        std::fs::write(
            temp_dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\n",
        )
        .unwrap();

        let output = run_helper(
            "agent::tests::reports_is_rust_project_for_subprocess",
            Some(temp_dir.path()),
            &[],
        );
        assert_eq!(extract_marker(&output, "IS_RUST_PROJECT="), "true");
    }

    #[test]
    fn detects_rust_project_from_explicit_dir() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().to_string_lossy().into_owned();

        std::fs::write(
            temp_dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\n",
        )
        .unwrap();

        assert!(is_rust_project(Some(&dir)));
    }

    #[test]
    fn returns_false_when_manifest_is_missing() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().to_string_lossy().into_owned();

        assert!(!is_rust_project(Some(&dir)));
    }

    #[test]
    fn shared_target_dir_prefers_explicit_env_var() {
        let _permit = test_subprocess::acquire();
        let temp_dir = TempDir::new().unwrap();
        let expected = temp_dir.path().join("shared-target");
        let output = run_helper(
            "agent::tests::reports_shared_target_dir_for_subprocess",
            None,
            &[("CARGO_TARGET_DIR", Some(expected.as_os_str()))],
        );
        assert_eq!(
            extract_marker(&output, "SHARED_TARGET_DIR="),
            expected.to_string_lossy()
        );
    }

    #[test]
    fn shared_target_dir_defaults_under_aid_home() {
        let _permit = test_subprocess::acquire();
        let temp_dir = TempDir::new().unwrap();
        let aid_home = temp_dir.path().join("aid-home");
        let expected = aid_home.join("cargo-target");
        let output = run_helper(
            "agent::tests::reports_shared_target_dir_for_subprocess",
            None,
            &[
                ("CARGO_TARGET_DIR", None),
                ("AID_HOME", Some(aid_home.as_os_str())),
            ],
        );
        assert_eq!(
            extract_marker(&output, "SHARED_TARGET_DIR="),
            expected.to_string_lossy()
        );
    }

    #[test]
    fn shared_target_dir_defaults_to_home_aid_path() {
        let _permit = test_subprocess::acquire();
        let temp_dir = TempDir::new().unwrap();
        let home_dir = temp_dir.path().join("home");
        let expected = PathBuf::from(&home_dir).join(".aid").join("cargo-target");
        let output = run_helper(
            "agent::tests::reports_shared_target_dir_for_subprocess",
            None,
            &[
                ("CARGO_TARGET_DIR", None),
                ("AID_HOME", None),
                ("HOME", Some(home_dir.as_os_str())),
            ],
        );
        assert_eq!(
            extract_marker(&output, "SHARED_TARGET_DIR="),
            expected.to_string_lossy()
        );
    }

    #[test]
    #[ignore]
    fn reports_is_rust_project_for_subprocess() {
        let _permit = test_subprocess::acquire();
        println!("IS_RUST_PROJECT={}", is_rust_project(None));
    }

    #[test]
    #[ignore]
    fn reports_shared_target_dir_for_subprocess() {
        let _permit = test_subprocess::acquire();
        println!(
            "SHARED_TARGET_DIR={}",
            shared_target_dir().unwrap_or_default()
        );
    }

    #[test]
    fn target_dir_for_worktree_isolates_branches() {
        let base = shared_target_dir().unwrap();
        let isolated = target_dir_for_worktree(Some("feat/my-feature")).unwrap();
        assert_eq!(isolated, format!("{base}/feat-my-feature"));
        let shared = target_dir_for_worktree(None).unwrap();
        assert_eq!(shared, base);
    }

    #[test]
    fn apply_run_env_sets_explicit_vars_on_command() {
        let mut cmd = Command::new("echo");
        let opts = RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: Some([("APP_MODE".to_string(), "test".to_string())].into_iter().collect()),
            env_forward: None,
        };

        apply_run_env(&mut cmd, &opts);

        let envs: Vec<_> = cmd.get_envs().collect();
        let mode = envs
            .iter()
            .find(|(key, _)| *key == "APP_MODE")
            .and_then(|(_, value)| value.as_ref())
            .map(|value| value.to_string_lossy().to_string());
        assert_eq!(mode.as_deref(), Some("test"));
    }

    #[test]
    fn apply_run_env_is_noop_for_empty_values() {
        let mut cmd = Command::new("echo");
        let opts = RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: Some(HashMap::new()),
            env_forward: Some(vec![]),
        };

        apply_run_env(&mut cmd, &opts);

        assert_eq!(cmd.get_envs().count(), 0);
    }

    #[test]
    fn apply_run_env_forwards_parent_vars() {
        let _permit = test_subprocess::acquire();
        let output = run_helper(
            "agent::tests::reports_forwarded_env_for_subprocess",
            None,
            &[("AID_TEST_FORWARDED_ENV", Some(OsStr::new("forwarded-value")))],
        );
        assert_eq!(
            extract_marker(&output, "FORWARDED_ENV="),
            "forwarded-value"
        );
    }

    #[test]
    #[ignore]
    fn reports_forwarded_env_for_subprocess() {
        let _permit = test_subprocess::acquire();
        let mut cmd = Command::new("echo");
        let opts = RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: Some(vec!["AID_TEST_FORWARDED_ENV".to_string()]),
        };
        apply_run_env(&mut cmd, &opts);
        let envs: Vec<_> = cmd.get_envs().collect();
        let forwarded = envs
            .iter()
            .find(|(key, _)| *key == "AID_TEST_FORWARDED_ENV")
            .and_then(|(_, value)| value.as_ref())
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        println!("FORWARDED_ENV={forwarded}");
    }

    fn run_helper(
        test_name: &str,
        current_dir: Option<&Path>,
        env_vars: &[(&str, Option<&OsStr>)],
    ) -> String {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", test_name, "--ignored", "--nocapture"]);
        if let Some(dir) = current_dir {
            cmd.current_dir(dir);
        }
        for (name, value) in env_vars {
            if let Some(value) = value {
                cmd.env(name, value);
            } else {
                cmd.env_remove(name);
            }
        }

        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "helper test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8(output.stdout).unwrap()
    }

    fn extract_marker<'a>(output: &'a str, prefix: &str) -> &'a str {
        output
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("missing marker {prefix} in output: {output}"))
    }
}
