// Agent trait and registry for AI CLI adapters.
// Each agent knows how to build its CLI command and parse its output.

pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod kilo;
pub mod opencode;
mod selection;
pub(crate) mod truncate;

use anyhow::Result;
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
}

/// Detect which agents are installed on the system
pub fn detect_agents() -> Vec<AgentKind> {
    let mut found = Vec::new();
    for (name, kind) in [
        ("gemini", AgentKind::Gemini),
        ("codex", AgentKind::Codex),
        ("opencode", AgentKind::OpenCode),
        ("cursor", AgentKind::Cursor),
        ("kilo", AgentKind::Kilo),
    ] {
        if which_exists(name) {
            found.push(kind);
        }
    }
    found
}

pub fn select_agent(prompt: &str, opts: &RunOpts, store: &store::Store) -> AgentKind {
    selection::select_agent_with_reason(prompt, opts, store).0
}

pub(crate) fn select_agent_with_reason(prompt: &str, opts: &RunOpts, store: &store::Store) -> (AgentKind, String) {
    let selection = selection::select_agent_with_reason(prompt, opts, store);
    debug_assert_eq!(select_agent(prompt, opts, store), selection.0);
    selection
}

/// Get an agent adapter by kind
pub fn get_agent(kind: AgentKind) -> Box<dyn Agent> {
    match kind {
        AgentKind::Codex => Box::new(codex::CodexAgent),
        AgentKind::Cursor => Box::new(cursor::CursorAgent),
        AgentKind::Gemini => Box::new(gemini::GeminiAgent),
        AgentKind::OpenCode => Box::new(opencode::OpenCodeAgent),
        AgentKind::Kilo => Box::new(kilo::KiloAgent),
    }
}

pub fn agent_has_fs_access(kind: &AgentKind) -> bool {
    !matches!(kind, AgentKind::Gemini)
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

pub fn is_rust_project(dir: Option<&str>) -> bool {
    resolve_project_dir(dir).join(CARGO_MANIFEST_NAME).is_file()
}

fn resolve_project_dir(dir: Option<&str>) -> PathBuf {
    dir.map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
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
    use super::{is_rust_project, shared_target_dir};
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn detects_rust_project_in_current_dir() {
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
        println!("IS_RUST_PROJECT={}", is_rust_project(None));
    }

    #[test]
    #[ignore]
    fn reports_shared_target_dir_for_subprocess() {
        println!(
            "SHARED_TARGET_DIR={}",
            shared_target_dir().unwrap_or_default()
        );
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
