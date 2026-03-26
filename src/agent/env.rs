// Agent environment helpers: shared target dirs, git ceiling, cwd resolution, run env.
// Exports: path and process helpers for agent runs. Deps: crate::paths, super::RunOpts.

use std::path::PathBuf;
use std::process::Command;

use crate::types::AgentKind;

use super::RunOpts;

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const CARGO_MANIFEST_NAME: &str = "Cargo.toml";
const SHARED_TARGET_DIR_NAME: &str = "cargo-target";

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
    cmd.env("AID_HOME", crate::paths::aid_dir());
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

pub(crate) fn which_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
