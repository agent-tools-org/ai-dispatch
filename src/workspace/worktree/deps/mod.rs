// Worktree dependency preparation: setup hooks, dep-dir symlinks, and verify hints.
// Exports: prepare_worktree_dependencies() and missing_deps_hint().
// Deps: crate::runtime::process::guard, crate::store, crate::types, std fs/process/path helpers.
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::runtime::process::guard::ProcessGuard;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

mod hint;
mod setup;
pub use hint::missing_deps_hint;
use setup::{capture_setup_output, join_setup_reader, poll_setup};

const SETUP_DONE_MARKER: &str = ".aid-setup-done";
const VERIFY_STATE_FILE: &str = ".aid-verify-deps-state";
const MAX_SCAN_DEPTH: usize = 3;
const MAX_SETUP_TIMEOUT_SECS: u64 = 600;
const MISSING_DEPS_HINT: &str = "Hint: verify likely failed because dependencies weren't installed in the fresh worktree. Define `[project] setup = \"npm ci\"` (or equivalent) in .aid/project.toml, or remove `verify`.";

pub fn prepare_worktree_dependencies(
    store: &Store,
    task_id: &TaskId,
    repo_dir: &Path,
    worktree_dir: &Path,
    setup: Option<&str>,
    link_deps: bool,
    idle_timeout_secs: Option<u64>,
    fresh: bool,
    worktree_branch: Option<&str>,
) -> Result<()> {
    maybe_seed_cargo_target(store, task_id, worktree_dir, worktree_branch);
    let setup_defined = setup.is_some();
    let linked_any = if let Some(command) = setup {
        maybe_run_setup(store, task_id, worktree_dir, command, idle_timeout_secs)?;
        false
    } else if link_deps {
        create_dep_symlinks(store, task_id, repo_dir, worktree_dir)?
    } else {
        false
    };
    write_verify_state(worktree_dir, fresh, setup_defined, linked_any)?;
    Ok(())
}

fn maybe_seed_cargo_target(
    store: &Store,
    task_id: &TaskId,
    worktree_dir: &Path,
    worktree_branch: Option<&str>,
) {
    if !crate::agent::is_rust_project(worktree_dir.to_str()) {
        return;
    }
    let Some(branch) = worktree_branch else {
        return;
    };
    let Some(outcome) = crate::agent::env::seed_branch_target_dir(worktree_dir.to_str(), branch)
    else {
        return;
    };
    match outcome {
        crate::agent::env::BranchTargetSeedOutcome::Seeded {
            target,
            source,
            elapsed_ms,
        } => {
            insert_setup_event(
                store,
                task_id,
                &format!("Cargo target seeded: {target} from {source} in {elapsed_ms}ms"),
            );
        }
        crate::agent::env::BranchTargetSeedOutcome::Skipped { target, reason } => {
            insert_setup_event(
                store,
                task_id,
                &format!("Cargo target seed skipped for {target}: {reason}"),
            );
        }
    }
}

fn maybe_run_setup(
    store: &Store,
    task_id: &TaskId,
    worktree_dir: &Path,
    command: &str,
    idle_timeout_secs: Option<u64>,
) -> Result<()> {
    let marker = worktree_dir.join(SETUP_DONE_MARKER);
    if marker.exists() {
        insert_setup_event(store, task_id, "Setup skipped: marker already present");
        return Ok(());
    }
    insert_setup_event(store, task_id, &format!("Running setup: {command}"));
    let timeout_secs = idle_timeout_secs
        .filter(|secs| *secs > 0)
        .unwrap_or(MAX_SETUP_TIMEOUT_SECS)
        .min(MAX_SETUP_TIMEOUT_SECS);
    let timeout = Duration::from_secs(timeout_secs);
    let mut cmd = Command::new("sh");
    cmd.args(["-c", command])
        .current_dir(worktree_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut guard = ProcessGuard::spawn(&mut cmd).context("Failed to start setup command")?;
    let (rx, stdout_thread, stderr_thread) = capture_setup_output(&mut guard)?;
    let status = poll_setup(store, task_id, &mut guard, &rx, timeout)?;
    join_setup_reader(stdout_thread)?;
    join_setup_reader(stderr_thread)?;
    while let Ok(message) = rx.try_recv() {
        if let Some(line) = message {
            insert_setup_event(store, task_id, &line);
        }
    }
    match status {
        Some(status) if status.success() => {
            fs::write(marker, format!("command={command}\n"))?;
            insert_setup_event(store, task_id, "Setup completed");
            Ok(())
        }
        Some(status) => anyhow::bail!("setup command failed with exit code {:?}", status.code()),
        None => anyhow::bail!("setup command timed out after {timeout_secs}s"),
    }
}

fn create_dep_symlinks(
    store: &Store,
    task_id: &TaskId,
    repo_dir: &Path,
    worktree_dir: &Path,
) -> Result<bool> {
    let mut linked_any = false;
    let mut seen = HashSet::new();
    for (parent, dep_name) in manifest_parents(repo_dir, repo_dir, 0, &mut seen)? {
        let rel = parent.strip_prefix(repo_dir).unwrap_or(parent.as_path());
        let source = repo_dir.join(rel).join(dep_name);
        let target = worktree_dir.join(rel).join(dep_name);
        if !source.exists() || target.exists() {
            continue;
        }
        if let Some(parent_dir) = target.parent() {
            fs::create_dir_all(parent_dir)?;
        }
        symlink_dir(&source, &target)?;
        linked_any = true;
        insert_setup_event(
            store,
            task_id,
            &format!("Linked {} -> {}", target.display(), source.display()),
        );
    }
    Ok(linked_any)
}

fn manifest_parents(
    repo_dir: &Path,
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<(PathBuf, &'static str)>,
) -> Result<Vec<(PathBuf, &'static str)>> {
    if depth > MAX_SCAN_DEPTH {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if matches!(
                name.as_ref(),
                ".git" | "node_modules" | "target" | ".venv" | "venv" | "vendor"
            ) {
                continue;
            }
            found.extend(manifest_parents(repo_dir, &path, depth + 1, seen)?);
            continue;
        }
        for dep_name in dep_names_for_manifest(name.as_ref()) {
            let key = (
                dir.strip_prefix(repo_dir).unwrap_or(dir).to_path_buf(),
                *dep_name,
            );
            if seen.insert(key) {
                found.push((dir.to_path_buf(), *dep_name));
            }
        }
    }
    Ok(found)
}

fn dep_names_for_manifest(name: &str) -> &'static [&'static str] {
    match name {
        "package.json" => &["node_modules"],
        "Cargo.toml" => &["target"],
        "pyproject.toml" | "requirements.txt" => &[".venv", "venv"],
        "Gemfile" => &["vendor"],
        _ => &[],
    }
}

#[cfg(unix)]
fn symlink_dir(source: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, target).with_context(|| {
        format!(
            "Failed to link {} -> {}",
            target.display(),
            source.display()
        )
    })
}

#[cfg(not(unix))]
fn symlink_dir(source: &Path, target: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(source, target).with_context(|| {
        format!(
            "Failed to link {} -> {}",
            target.display(),
            source.display()
        )
    })
}

fn write_verify_state(
    worktree_dir: &Path,
    fresh: bool,
    setup_defined: bool,
    linked_any: bool,
) -> Result<()> {
    let path = worktree_dir.join(VERIFY_STATE_FILE);
    let mut file = File::create(path)?;
    use std::io::Write;
    writeln!(file, "fresh={}", if fresh { 1 } else { 0 })?;
    writeln!(file, "setup_defined={}", if setup_defined { 1 } else { 0 })?;
    writeln!(file, "linked_any={}", if linked_any { 1 } else { 0 })?;
    Ok(())
}

fn insert_setup_event(store: &Store, task_id: &TaskId, detail: &str) {
    let _ = store.insert_event(&TaskEvent::new(task_id.clone(), EventKind::Setup, detail.to_string()));
}

#[cfg(test)]
#[path = "worktree_deps_tests.rs"]
mod tests;
