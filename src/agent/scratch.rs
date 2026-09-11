// Prepare and probe writable directories before launching agent processes.
// Exports scratch environment and Codex/Copilot grant helpers.
// Deps: std filesystem APIs, rand, anyhow, git layout helpers, and agent RunOpts.

use std::fs;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use crate::types::AgentKind;
use crate::worktree_layout::{read_commondir, resolve_worktree_gitdir};
use super::RunOpts;

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

fn writable_roots(dir: Option<&str>, target: Option<&Path>) -> Result<Vec<PathBuf>> {
    let dir = dir.map(PathBuf::from).map(Ok).unwrap_or_else(std::env::current_dir)?;
    let git = dir.join(".git");
    let gitdir = if git.is_dir() { Some(git) } else { resolve_worktree_gitdir(&dir) };
    let mut roots = Vec::new();
    if let Some(gitdir) = gitdir {
        roots.push(gitdir.clone());
        if let Some(common) = read_commondir(&gitdir) {
            roots.push(common);
        }
    }
    roots.extend(target.map(Path::to_path_buf));
    Ok(roots)
}

fn prepare_roots(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for path in paths {
        ensure_directory(&path)?;
        probe_directory(&path)?;
        let path = path.canonicalize()?;
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    Ok(roots)
}

pub(crate) fn grant_codex_roots(
    cmd: &mut Command, dir: Option<&str>, target: Option<&Path>, temp: Option<&Path>,
) -> Result<()> {
    let mut roots = writable_roots(dir, target)?;
    roots.extend(temp.map(Path::to_path_buf));
    let roots = prepare_roots(roots)?;
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

pub(crate) fn grant_launch_dirs(
    cmd: &mut Command, kind: AgentKind, opts: &RunOpts, target: Option<&str>,
) -> Result<()> {
    let temp = cmd.get_envs().find_map(|(key, value)| {
        (key == "TMPDIR").then_some(value).flatten().map(PathBuf::from)
    }).context("task TMPDIR was not prepared")?;
    let mut roots = vec![temp];
    if kind == AgentKind::Copilot {
        roots.extend(writable_roots(opts.dir.as_deref(), target.map(Path::new))?);
        roots.extend(opts.dir.as_deref().map(PathBuf::from));
        roots.extend(opts.context_files.iter().filter_map(|file| {
            Path::new(file).parent().filter(|path| !path.as_os_str().is_empty()).map(Path::to_path_buf)
        }));
    }
    for root in prepare_roots(roots)? {
        if kind == AgentKind::Copilot {
            cmd.arg("--add-dir").arg(root);
        }
    }
    Ok(())
}

pub(crate) fn mount_scratch_dirs(wrapped: &mut Command, cmd: &Command) {
    for path in command_scratch_dirs(cmd) {
        wrapped.arg("-v").arg(format!("{}:{}", path.display(), path.display()));
    }
}

fn command_scratch_dirs(cmd: &Command) -> Vec<PathBuf> {
    cmd.get_envs().filter_map(|(key, value)| {
        if key == "TMPDIR" || key == "CARGO_TARGET_DIR" {
            value.map(PathBuf::from)
        } else {
            None
        }
    }).collect()
}

pub(crate) fn prepare_container_scratch(cmd: &Command, container: &str) -> Result<()> {
    let paths = command_scratch_dirs(cmd);
    for path in paths {
        let output = container_probe_command(container, &path).output()
            .with_context(|| format!("cannot probe granted directory '{}' in container '{container}'", path.display()))?;
        anyhow::ensure!(output.status.success(),
            "cannot prepare granted directory '{}' in container '{}': {}",
            path.display(), container, String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

fn container_probe_command(container: &str, path: &Path) -> Command {
    let mut cmd = Command::new("container");
    cmd.args(["exec", container, "sh", "-c",
        "mkdir -p -- \"$1\" && (umask 077; set -C; echo aid > \"$1/$2\") && rm -- \"$1/$2\"",
        "aid-scratch-probe"]);
    cmd.arg(path).arg(format!(".aid-write-probe-{:032x}", rand::random::<u128>()));
    cmd
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod tests;
