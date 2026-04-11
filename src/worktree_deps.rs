// Worktree dependency preparation: setup hooks, dep-dir symlinks, and verify hints.
// Exports: prepare_worktree_dependencies() and missing_deps_hint().
// Deps: crate::process_guard, crate::store, crate::types, std fs/process/path helpers.

use anyhow::{Context, Result};
use chrono::Local;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::process_guard::ProcessGuard;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

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
) -> Result<()> {
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

pub fn missing_deps_hint(worktree_dir: &Path) -> Option<&'static str> {
    let state = fs::read_to_string(worktree_dir.join(VERIFY_STATE_FILE)).ok()?;
    let mut fresh = false;
    let mut setup_defined = false;
    let mut linked_any = false;
    for line in state.lines() {
        if let Some(value) = line.strip_prefix("fresh=") {
            fresh = value == "1";
        } else if let Some(value) = line.strip_prefix("setup_defined=") {
            setup_defined = value == "1";
        } else if let Some(value) = line.strip_prefix("linked_any=") {
            linked_any = value == "1";
        }
    }
    (fresh && !setup_defined && !linked_any).then_some(MISSING_DEPS_HINT)
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
    let stdout = guard
        .child_mut()
        .stdout
        .take()
        .context("setup stdout pipe missing")?;
    let stderr = guard
        .child_mut()
        .stderr
        .take()
        .context("setup stderr pipe missing")?;
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let stdout_thread = spawn_setup_reader(stdout, tx.clone());
    let stderr_thread = spawn_setup_reader(stderr, tx);
    let start = Instant::now();
    let status = loop {
        while let Ok(message) = rx.try_recv() {
            if let Some(line) = message {
                insert_setup_event(store, task_id, &line);
            }
        }
        if let Some(status) = guard
            .child_mut()
            .try_wait()
            .context("Failed to poll setup command")?
        {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            guard.force_kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
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

fn spawn_setup_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: mpsc::Sender<Option<String>>,
) -> std::thread::JoinHandle<Result<()>> {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let line = line?;
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let _ = tx.send(Some(trimmed.to_string()));
            }
        }
        Ok(())
    })
}

fn join_setup_reader(handle: std::thread::JoinHandle<Result<()>>) -> Result<()> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("setup output reader thread panicked"))?
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
            if matches!(name.as_ref(), ".git" | "node_modules" | "target" | ".venv" | "venv" | "vendor") {
                continue;
            }
            found.extend(manifest_parents(repo_dir, &path, depth + 1, seen)?);
            continue;
        }
        for dep_name in dep_names_for_manifest(name.as_ref()) {
            let key = (dir.strip_prefix(repo_dir).unwrap_or(dir).to_path_buf(), *dep_name);
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
    std::os::unix::fs::symlink(source, target)
        .with_context(|| format!("Failed to link {} -> {}", target.display(), source.display()))
}

#[cfg(not(unix))]
fn symlink_dir(source: &Path, target: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
        .with_context(|| format!("Failed to link {} -> {}", target.display(), source.display()))
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
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Setup,
        detail: detail.to_string(),
        metadata: None,
    });
}

#[cfg(test)]
#[path = "worktree_deps_tests.rs"]
mod tests;
