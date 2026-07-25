// Agent environment helpers: shared target dirs, git ceiling, cwd resolution, run env.
// Exports: path and process helpers for agent runs.
// Deps: crate::paths, std::process::Command, super::RunOpts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::types::AgentKind;

use super::RunOpts;

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const CARGO_MANIFEST_NAME: &str = "Cargo.toml";
const BASE_TARGET_DIR_NAME: &str = "_base";
const SHARED_TARGET_DIR_NAME: &str = "cargo-target";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BranchTargetSeedOutcome {
    Seeded { target: String, source: String, elapsed_ms: u128 },
    Skipped { target: String, reason: String },
}

struct CargoTargetLayout {
    source: PathBuf,
    branch_root: PathBuf,
}

pub fn agent_has_fs_access(_kind: &AgentKind) -> bool {
    true // all supported agents have file system access
}

pub fn shared_target_dir() -> Option<String> {
    Some(target_layout()?.source.to_string_lossy().into_owned())
}

fn target_layout() -> Option<CargoTargetLayout> {
    if let Some(source) = std::env::var_os(CARGO_TARGET_DIR_ENV).map(PathBuf::from) {
        let branch_root = source
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        return Some(CargoTargetLayout { source, branch_root });
    }
    let branch_root = crate::paths::aid_dir().join(SHARED_TARGET_DIR_NAME);
    let source = branch_root.join(BASE_TARGET_DIR_NAME);
    Some(CargoTargetLayout { source, branch_root })
}

/// Returns a target directory isolated per worktree branch.
/// Worktree tasks get `{base}/{sanitized_branch}` to avoid lock contention.
/// Non-worktree tasks share the base directory.
pub fn target_dir_for_worktree(worktree_branch: Option<&str>) -> Option<String> {
    let layout = target_layout()?;
    match worktree_branch {
        Some(branch) => {
            let target = target_dir_for_branch(&layout.branch_root, branch);
            Some(target.to_string_lossy().into_owned())
        }
        None => shared_target_dir(),
    }
}

pub(crate) fn seed_branch_target_dir(
    worktree_branch: &str,
) -> Option<BranchTargetSeedOutcome> {
    let layout = target_layout()?;
    let target = target_dir_for_branch(&layout.branch_root, worktree_branch);
    Some(seed_branch_target_from_source(&layout.source, &target))
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
        apply_cargo_target_env(cmd, Some(target_dir.as_str()));
    }
}

pub fn apply_cargo_target_env(cmd: &mut Command, cargo_target_dir: Option<&str>) {
    if let Some(target_dir) = cargo_target_dir {
        cmd.env(CARGO_TARGET_DIR_ENV, target_dir);
    }
}

pub fn cargo_target_env_arg(cargo_target_dir: &str) -> String {
    format!("{CARGO_TARGET_DIR_ENV}={cargo_target_dir}")
}

fn target_dir_for_branch(base: &Path, branch: &str) -> PathBuf {
    base.join(branch.replace('/', "-"))
}

fn seed_branch_target_from_source(source: &Path, target: &Path) -> BranchTargetSeedOutcome {
    let target_display = target.to_string_lossy().into_owned();
    if target.exists() {
        return skipped(target_display, "destination exists");
    }
    if !source.is_dir() {
        return skipped(target_display, "base target directory is missing");
    }
    let Some(parent) = target.parent() else {
        return skipped(target_display, "destination has no parent directory");
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        return skipped(target_display, format!("failed to create parent directory: {err}"));
    }
    let temp_target = temp_seed_target(target);
    let start = Instant::now();
    let output = Command::new("cp")
        .arg("-Rc")
        .arg(source)
        .arg(&temp_target)
        .stdin(Stdio::null())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => return skipped(target_display, format!("failed to start clone copy: {err}")),
    };
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&temp_target);
        return skipped(target_display, format!("clone copy failed: {}", copy_stderr(&output.stderr)));
    }
    if let Err(err) = std::fs::rename(&temp_target, target) {
        let _ = std::fs::remove_dir_all(&temp_target);
        return skipped(target_display, format!("failed to move seeded target into place: {err}"));
    }
    BranchTargetSeedOutcome::Seeded {
        target: target_display,
        source: source.to_string_lossy().into_owned(),
        elapsed_ms: start.elapsed().as_millis(),
    }
}

fn temp_seed_target(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("target");
    target.with_file_name(format!(".{name}.seed-{}", std::process::id()))
}

fn skipped(target: String, reason: impl Into<String>) -> BranchTargetSeedOutcome {
    BranchTargetSeedOutcome::Skipped { target, reason: reason.into() }
}

fn copy_stderr(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_string();
    if message.is_empty() {
        "cp -Rc exited unsuccessfully".to_string()
    } else {
        message
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

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
