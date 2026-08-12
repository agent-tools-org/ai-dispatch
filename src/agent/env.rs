// Agent environment helpers: shared target dirs, git ceiling, cwd resolution, run env.
// Exports: path and process helpers for agent runs.
// Deps: anyhow::Result, crate::paths, std::process::Command, super::RunOpts.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Result;
use crate::types::AgentKind;

use super::RunOpts;
pub(crate) use super::cargo_target::BranchTargetSeedOutcome;

#[derive(Debug)]
pub(crate) struct CliCommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) type CliCommandRunner<'a> = dyn Fn(&str, &[&str]) -> Result<CliCommandOutput> + 'a;

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const CARGO_MANIFEST_NAME: &str = "Cargo.toml";
const BASE_TARGET_DIR_NAME: &str = "_base";
const SHARED_TARGET_DIR_NAME: &str = "cargo-target";

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
    if let Some(branch_root) = std::env::var_os(CARGO_TARGET_DIR_ENV).map(PathBuf::from) {
        let source = branch_root.join(BASE_TARGET_DIR_NAME);
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

pub(crate) fn branch_target_root() -> Option<PathBuf> {
    Some(target_layout()?.branch_root)
}

pub(crate) fn branch_target_name(branch: &str) -> String {
    branch.replace('/', "-")
}

pub(crate) fn seed_branch_target_dir(
    worktree_branch: &str,
) -> Option<BranchTargetSeedOutcome> {
    let layout = target_layout()?;
    let target = target_dir_for_branch(&layout.branch_root, worktree_branch);
    let outcome = if layout.source.is_dir() {
        super::cargo_target::seed_branch_target_from_source(&layout.source, &target)
    } else {
        super::cargo_target::seed_branch_target_from_root_entries(&layout.branch_root, &target)
    };
    let outcome = ensure_branch_target_dir(&target, outcome);
    Some(outcome)
}

fn ensure_branch_target_dir(
    target: &Path,
    outcome: BranchTargetSeedOutcome,
) -> BranchTargetSeedOutcome {
    if target.is_dir() {
        return outcome;
    }
    let target_display = target.to_string_lossy().into_owned();
    match std::fs::create_dir_all(target) {
        Ok(()) => outcome,
        Err(err) => BranchTargetSeedOutcome::Skipped {
            target: target_display,
            reason: format!("seed did not create target directory: {err}"),
        },
    }
}

pub(crate) fn remove_branch_target_dir_if_unused(repo_dir: &Path, branch: &str) -> anyhow::Result<bool> {
    let Some(layout) = target_layout() else {
        return Ok(false);
    };
    let target = target_dir_for_branch(&layout.branch_root, branch);
    if target.file_name().and_then(|name| name.to_str()) == Some(BASE_TARGET_DIR_NAME) {
        return Ok(false);
    }
    if branch_has_live_worktree(repo_dir, branch)? {
        return Ok(false);
    }
    super::cargo_target::remove_branch_target_dir(&target)
}

pub fn apply_rust_build_cache_env(
    cmd: &mut Command,
    project_dir: Option<&str>,
    worktree_branch: Option<&str>,
) {
    if let Some(target_dir) = rust_build_cache_target_dir(project_dir, worktree_branch) {
        apply_cargo_target_env(cmd, Some(target_dir.as_str()));
    }
}

pub fn rust_build_cache_target_dir(
    project_dir: Option<&str>,
    worktree_branch: Option<&str>,
) -> Option<String> {
    if !is_rust_project(project_dir) {
        return None;
    }
    target_dir_for_worktree(worktree_branch)
}

pub fn apply_cargo_target_env(cmd: &mut Command, cargo_target_dir: Option<&str>) {
    if let Some(target_dir) = cargo_target_dir {
        cmd.env(CARGO_TARGET_DIR_ENV, target_dir);
    }
}

pub fn apply_codex_home_env(cmd: &mut Command) -> anyhow::Result<()> {
    let codex_home = super::home_isolation::resolve_real_home()?.join(".codex");
    cmd.env("CODEX_HOME", codex_home);
    Ok(())
}

pub(crate) fn should_use_durable_codex_home(
    agent_kind: AgentKind,
    sandbox: bool,
    containerized: bool,
) -> bool {
    agent_kind == AgentKind::Codex && !sandbox && !containerized
}

pub fn cargo_target_env_arg(cargo_target_dir: &str) -> String {
    format!("{CARGO_TARGET_DIR_ENV}={cargo_target_dir}")
}

fn target_dir_for_branch(base: &Path, branch: &str) -> PathBuf {
    base.join(branch_target_name(branch))
}

fn branch_has_live_worktree(repo_dir: &Path, branch: &str) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()?;
    if !output.status.success() {
        return Ok(true);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(worktree_list_has_branch(&stdout, branch))
}

fn worktree_list_has_branch(output: &str, branch: &str) -> bool {
    output.split("\n\n").any(|block| {
        !block.lines().any(|line| line.starts_with("prunable "))
            && block.lines().any(|line| {
                line.strip_prefix("branch refs/heads/") == Some(branch)
            })
    })
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

pub fn apply_run_env(
    cmd: &mut Command,
    opts: &RunOpts,
    task_id: Option<&str>,
) -> anyhow::Result<super::home_isolation::IsolatedHomeGuard> {
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
    let guard = super::home_isolation::IsolatedHomeGuard::create(task_id)?;
    guard.apply_toolchain_env(cmd);
    cmd.env("HOME", guard.path());
    Ok(guard)
}

pub(crate) fn which_exists(name: &str) -> bool {
    if identity_marker(name).is_some() {
        return binary_identity_matches(name);
    }
    let on_path = Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    on_path
}

fn binary_identity_matches(name: &str) -> bool {
    let Some(marker) = identity_marker(name) else {
        return true;
    };
    let mut command = Command::new(name);
    command.arg("--help").stdout(Stdio::piped()).stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                let stdout_ok = child
                    .stdout
                    .take()
                    .is_some_and(|mut stream| stream.read_to_end(&mut stdout).is_ok());
                let stderr_ok = child
                    .stderr
                    .take()
                    .is_some_and(|mut stream| stream.read_to_end(&mut stderr).is_ok());
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
                let identity = text.to_ascii_lowercase().contains(marker);
                return status.success() && stdout_ok && stderr_ok && identity;
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => return false,
        }
    }
}

fn identity_marker(name: &str) -> Option<&'static str> {
    match name {
        "agent" => Some("cursor"),
        "claude" => Some("claude code"),
        "oz" => Some("warp"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
