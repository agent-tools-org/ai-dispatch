// Prepare and probe writable directories before launching agent processes.
// Exports scratch environment and Codex/Copilot grant helpers.
// Deps: std filesystem APIs, rand, anyhow, git layout helpers, and task events.

use std::fs;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId};
use crate::worktree_layout::{read_commondir, resolve_worktree_gitdir};

pub(crate) fn ensure_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("cannot create granted directory '{}'", path.display()))
}

fn probe_directory(path: &Path) -> Result<()> {
    let probe = || -> Result<()> {
        anyhow::ensure!(!fs::metadata(path)?.permissions().readonly(), "directory is read-only");
        let probe = path.join(format!(".aid-write-probe-{:032x}", rand::random::<u128>()));
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
        let written = file.write_all(b"aid write probe");
        drop(file);
        let removed = fs::remove_file(&probe);
        written?;
        removed?;
        Ok(())
    };
    probe().with_context(|| format!("cannot write to granted directory '{}'", path.display()))
}

pub(crate) fn prepare_cargo_target(target: Option<&str>) -> Result<Option<String>> {
    if let Some(target) = target {
        let path = Path::new(target);
        ensure_directory(path)?;
        probe_directory(path)?;
        let canonical = path.canonicalize()?;
        return Ok(Some(canonical.to_str().with_context(||
            format!("granted directory is not valid UTF-8: '{}'", canonical.display()))?.to_string()));
    }
    Ok(None)
}

pub(crate) fn create_temp_dir(home: &Path) -> Result<PathBuf> {
    let temp = home.join(format!(".aid-tmp-{:032x}", rand::random::<u128>()));
    fs::DirBuilder::new().mode(0o700).create(&temp)
        .with_context(|| format!("cannot create task temporary directory '{}'", temp.display()))?;
    probe_directory(&temp)?;
    Ok(temp.canonicalize()?)
}

pub(crate) fn writable_roots(dir: Option<&str>) -> Vec<PathBuf> {
    let Some(dir) = dir.map(Path::new) else { return Vec::new() };
    let git = dir.join(".git");
    let gitdir = if git.is_dir() { Some(git) } else { resolve_worktree_gitdir(dir) };
    let mut roots = Vec::new();
    if let Some(gitdir) = gitdir {
        roots.push(gitdir.clone());
        if let Some(common) = read_commondir(&gitdir) {
            roots.push(common);
        }
    }
    roots
}

pub(crate) fn prepare_launch_roots(
    kind: AgentKind, dir: Option<&str>, target: Option<&str>, temp: &Path, task_id: &TaskId,
) -> Result<(Vec<PathBuf>, Vec<TaskEvent>)> {
    let mut roots = Vec::new();
    let mut warnings = Vec::new();
    if matches!(kind, AgentKind::Codex | AgentKind::Copilot) {
        let cwd = dir.map(PathBuf::from).map(Ok).unwrap_or_else(std::env::current_dir)?;
        for path in writable_roots(cwd.to_str()) {
            let prepared = path.canonicalize().map_err(anyhow::Error::from)
                .and_then(|path| probe_directory(&path).map(|()| path));
            match prepared {
                Ok(path) => {
                    if !roots.contains(&path) { roots.push(path); }
                }
                Err(err) => warnings.push(TaskEvent {
                    task_id: task_id.clone(), timestamp: chrono::Local::now(),
                    event_kind: EventKind::Setup,
                    detail: format!("Omitting writable Git metadata grant '{}': {err:#}", path.display()),
                    metadata: Some(serde_json::json!({ "warning": "sandbox_root_omitted", "directory": path })),
                }),
            }
        }
    }
    for path in target.map(Path::new).into_iter().chain(std::iter::once(temp)) {
        if !roots.iter().any(|root| root == path) { roots.push(path.to_path_buf()); }
    }
    Ok((roots, warnings))
}

pub(crate) fn grant_codex_roots(cmd: &mut Command, roots: &[PathBuf]) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let roots = roots.iter().map(|path| {
        path.to_str().map(|path| toml::Value::String(path.to_string()))
            .with_context(|| format!("granted directory is not valid UTF-8: '{}'", path.display()))
    }).collect::<Result<Vec<_>>>()?;
    cmd.args(["-c", &format!("sandbox_workspace_write.writable_roots={}", toml::Value::Array(roots))]);
    Ok(())
}

pub(crate) fn grant_launch_dirs(mut cmd: Command, kind: AgentKind, roots: &[PathBuf]) -> Command {
    if kind == AgentKind::Copilot {
        for root in roots {
            cmd.arg("--add-dir").arg(root);
        }
    }
    cmd
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod tests;
