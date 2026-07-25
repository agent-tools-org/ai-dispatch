// Agent environment helpers: shared target dirs, git ceiling, cwd resolution, run env.
// Exports: path and process helpers for agent runs. Deps: crate::paths, super::RunOpts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::types::AgentKind;

use super::RunOpts;

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const CARGO_MANIFEST_NAME: &str = "Cargo.toml";
const RUSTC_WRAPPER_ENV: &str = "RUSTC_WRAPPER";
const SCCACHE_BIN: &str = "sccache";
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
            let target = target_dir_for_branch(Path::new(&base), branch);
            seed_branch_target_dir(Path::new(&base), &target);
            Some(target.to_string_lossy().into_owned())
        }
        None => Some(base),
    }
}

pub fn apply_rust_build_cache_env(
    cmd: &mut Command,
    project_dir: Option<&str>,
    worktree_branch: Option<&str>,
) {
    if !is_rust_project(project_dir) {
        return;
    }
    if let Some(target_dir) = target_dir_for_worktree(worktree_branch) {
        cmd.env(CARGO_TARGET_DIR_ENV, target_dir);
    }
    if std::env::var_os(RUSTC_WRAPPER_ENV).is_none()
        && !command_has_env(cmd, RUSTC_WRAPPER_ENV)
        && which_exists(SCCACHE_BIN)
    {
        cmd.env(RUSTC_WRAPPER_ENV, SCCACHE_BIN);
    }
}

fn target_dir_for_branch(base: &Path, branch: &str) -> PathBuf {
    base.join(branch.replace('/', "-"))
}

fn seed_branch_target_dir(base: &Path, target: &Path) {
    if target.exists() || !base.is_dir() {
        return;
    }
    let _ = Command::new("cp")
        .arg("-Rc")
        .arg(base)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn command_has_env(cmd: &Command, name: &str) -> bool {
    cmd.get_envs().any(|(key, _)| key == name)
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

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
