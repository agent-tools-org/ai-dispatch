// Stage native Codex read-only artifacts separately from trusted task writers.
// Exports staged paths, destination comparison and atomic delivery after exit.
// Deps: RunArgs, task paths, protected roots and filesystem operations.

use super::RunArgs;
use crate::types::{AgentKind, Task};
use anyhow::{Context, Result};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
};

pub(crate) fn enabled(args: &RunArgs, kind: AgentKind) -> bool {
    args.read_only && kind == AgentKind::Codex && !args.sandbox && args.container.is_none()
}

pub(crate) fn prepare(args: &mut RunArgs, kind: AgentKind, task: &Task) -> Result<()> {
    if !enabled(args, kind) { return Ok(()); }
    let staging = crate::agent::scratch::create_temp_dir(&std::env::temp_dir())?;
    let task_dir = canonical_path(&crate::paths::task_dir(task.id.as_str()))?;
    let cwd = resolve(".", task.effective_dir.as_deref().or(args.dir.as_deref())).canonicalize()?;
    anyhow::ensure!(!same_destination(args, task.effective_dir.as_deref())?, "result/-o: same destination");
    let mut forbidden = crate::agent::scratch::protected_roots(&cwd)?;
    anyhow::ensure!(!forbidden.iter().chain([&task_dir])
        .any(|root| staging.starts_with(root) || root.starts_with(&staging)),
        "read-only staging directory overlaps repository or trusted task directory");
    forbidden.push(staging.clone());
    validate_destinations(args, &forbidden, task.effective_dir.as_deref().or(args.dir.as_deref()))?;
    args.artifact_staging = Some(staging.display().to_string());
    Ok(())
}

pub(crate) fn validate_destinations(args: &RunArgs, forbidden: &[PathBuf], dir: Option<&str>) -> Result<()> {
    for destination in [args.output.as_deref(), args.result_file.as_deref()].into_iter().flatten() {
        let path = canonical_path(&resolve(destination, dir))?;
        anyhow::ensure!(!forbidden.iter().any(|root| path.starts_with(root)),
            "artifact destination '{}' is inside a repository or Codex-writable root", path.display());
    }
    Ok(())
}

pub(crate) fn agent_paths(
    args: &RunArgs,
    kind: AgentKind,
) -> (Option<String>, Option<String>) {
    if !enabled(args, kind) {
        return (args.output.clone(), args.result_file.clone());
    }
    let staging = args.artifact_staging.as_deref().map(Path::new);
    (
        args.output
            .as_ref().zip(staging)
            .map(|(_, staging)| staging.join("agent-output.txt").display().to_string()),
        args.result_file
            .as_ref().zip(staging)
            .map(|(_, staging)| staging.join("agent-result.md").display().to_string()),
    )
}

pub(crate) fn publish(
    args: &RunArgs,
    kind: AgentKind,
    dir: Option<&str>,
) -> Result<()> {
    if !enabled(args, kind) {
        return Ok(());
    }
    let (output, result) = agent_paths(args, kind);
    for (source, destination) in [
        (output.as_deref(), args.output.as_deref()),
        (result.as_deref(), args.result_file.as_deref()),
    ] {
        let (Some(source), Some(destination)) = (source, destination) else {
            continue;
        };
        let metadata = match fs::symlink_metadata(source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(metadata.is_file(), "staged artifact '{}' is not a regular file", source);
        let content = fs::read_to_string(source)?;
        deliver(&content, &resolve(destination, dir))
            .with_context(|| format!("Cannot deliver read-only artifact to '{}'", destination))?;
    }
    Ok(())
}

pub(crate) fn same_destination(args: &RunArgs, dir: Option<&str>) -> Result<bool> {
    let (Some(output), Some(report)) = (&args.output, &args.result_file) else {
        return Ok(false);
    };
    Ok(canonical_path(&resolve(output, None))?
        == canonical_path(&resolve(report, dir))?)
}

fn deliver(content: &str, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("Artifact destination needs a parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".aid-delivery-{:032x}", rand::random::<u128>()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(content.as_bytes())?;
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn canonical_path(path: &Path) -> Result<PathBuf> {
    let mut resolved = std::env::current_dir()?;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => resolved = prefix.as_os_str().into(),
            Component::RootDir => resolved = PathBuf::from("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                if !fs::symlink_metadata(&resolved)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
                    resolved = resolved.canonicalize()?;
                }
            }
        }
    }
    Ok(resolved)
}

fn resolve(path: &str, dir: Option<&str>) -> PathBuf {
    Path::new(dir.unwrap_or(".")).join(path)
}
