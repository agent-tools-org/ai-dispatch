// Stage native Codex read-only artifacts inside the granted task directory.
// Exports staged paths, destination comparison and atomic delivery after exit.
// Deps: RunArgs, task paths, protected roots and filesystem operations.

use super::RunArgs;
use crate::types::AgentKind;
use anyhow::{Context, Result};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
};

fn enabled(args: &RunArgs, kind: AgentKind) -> bool {
    args.read_only && kind == AgentKind::Codex && !args.sandbox && args.container.is_none()
}

pub(crate) fn agent_paths(
    args: &RunArgs,
    kind: AgentKind,
    task_id: &str,
) -> (Option<String>, Option<String>) {
    if !enabled(args, kind) {
        return (args.output.clone(), args.result_file.clone());
    }
    let task = crate::paths::task_dir(task_id);
    (
        args.output
            .as_ref()
            .map(|_| task.join("agent-output.txt").display().to_string()),
        args.result_file
            .as_ref()
            .map(|_| task.join("agent-result.md").display().to_string()),
    )
}

pub(crate) fn publish(
    args: &RunArgs,
    kind: AgentKind,
    task_id: &str,
    dir: Option<&str>,
) -> Result<()> {
    if !enabled(args, kind) {
        return Ok(());
    }
    let protected = crate::agent::scratch::protected_roots(&resolve(".", dir).canonicalize()?)?;
    let (output, result) = agent_paths(args, kind, task_id);
    for (source, destination) in [
        (output.as_deref(), args.output.as_deref()),
        (result.as_deref(), args.result_file.as_deref()),
    ] {
        let (Some(source), Some(destination)) = (source, destination) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(source) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        deliver(&content, &resolve(destination, dir), &protected)
            .with_context(|| format!("Cannot deliver read-only artifact to '{}'", destination))?;
    }
    Ok(())
}

pub(crate) fn same_destination(args: &RunArgs) -> Result<bool> {
    let (Some(output), Some(report)) = (&args.output, &args.result_file) else {
        return Ok(false);
    };
    Ok(canonical_path(&resolve(output, None))?
        == canonical_path(&resolve(report, args.dir.as_deref()))?)
}

fn deliver(content: &str, destination: &Path, protected: &[PathBuf]) -> Result<()> {
    let parent = destination
        .parent()
        .context("Artifact destination needs a parent directory")?;
    let name = destination
        .file_name()
        .context("Artifact destination needs a file name")?;
    let parent = canonical_path(parent)?;
    let destination = parent.join(name);
    for path in [&destination, &canonical_path(&destination)?] {
        anyhow::ensure!(
            !protected.iter().any(|root| path.starts_with(root)),
            "artifact destination '{}' is inside a protected repository path",
            path.display()
        );
    }
    fs::create_dir_all(&parent)?;
    let parent = parent.canonicalize()?;
    anyhow::ensure!(
        !protected.iter().any(|root| parent.starts_with(root)),
        "artifact parent '{}' is inside a protected repository path",
        parent.display()
    );
    let destination = parent.join(name);
    let temporary = parent.join(format!(".aid-delivery-{:032x}", rand::random::<u128>()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(content.as_bytes())?;
        drop(file);
        fs::rename(&temporary, &destination)?;
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
                match resolved.canonicalize() {
                    Ok(path) => resolved = path,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound
                            && fs::symlink_metadata(&resolved).is_err_and(|error| {
                                error.kind() == std::io::ErrorKind::NotFound
                            }) => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(resolved)
}

fn resolve(path: &str, dir: Option<&str>) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(dir.unwrap_or(".")).join(path)
    }
}
