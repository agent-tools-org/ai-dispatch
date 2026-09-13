// Best-of artifact helpers for output and result-file isolation.
// Exports candidate artifact derivation and winner/loser file finalization.

use anyhow::{Context, Result};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use crate::types::TaskId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DispatchArtifacts {
    pub(super) output: Option<String>,
    pub(super) result_file: Option<String>,
}

pub(super) fn dispatch_artifacts_for_candidate(
    output: Option<&str>,
    result_file: Option<&str>,
    candidate_idx: usize,
) -> DispatchArtifacts {
    if candidate_idx == 0 {
        return DispatchArtifacts {
            output: output.map(str::to_string),
            result_file: result_file.map(str::to_string),
        };
    }
    let suffix = format!("-bo{}", candidate_idx + 1);
    DispatchArtifacts {
        output: output.map(|path| suffixed_path(path, &suffix)),
        result_file: result_file.map(|path| suffixed_path(path, &suffix)),
    }
}

pub(super) fn suffixed_path(path: &str, suffix: &str) -> String {
    let path_buf = Path::new(path);
    let Some(stem) = path_buf.file_stem() else {
        return format!("{path}{suffix}");
    };
    let mut file_name = stem.to_os_string();
    file_name.push(suffix);
    if let Some(ext) = path_buf.extension() {
        file_name.push(".");
        file_name.push(ext);
    }
    match path_buf.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        Some(parent) => parent.join(file_name).to_string_lossy().into_owned(),
        None => PathBuf::from(file_name).to_string_lossy().into_owned(),
    }
}

pub(super) fn finalize_winner_artifacts(
    original: &DispatchArtifacts,
    artifacts_by_task: &[(TaskId, DispatchArtifacts)],
    winner_task_id: &TaskId,
) -> Result<()> {
    for (task_id, artifacts) in artifacts_by_task {
        if task_id == winner_task_id {
            copy_artifact(artifacts.output.as_deref(), original.output.as_deref())?;
            copy_artifact(
                artifacts.result_file.as_deref(),
                original.result_file.as_deref(),
            )?;
            continue;
        }
        cleanup_artifact(artifacts.output.as_deref(), original.output.as_deref())?;
        cleanup_artifact(
            artifacts.result_file.as_deref(),
            original.result_file.as_deref(),
        )?;
    }
    Ok(())
}

fn copy_artifact(source: Option<&str>, destination: Option<&str>) -> Result<()> {
    let (Some(source), Some(destination)) = (source, destination) else {
        return Ok(());
    };
    if source == destination {
        return Ok(());
    }
    std::fs::copy(source, destination)
        .with_context(|| format!("Failed to copy best-of artifact {source} to {destination}"))?;
    Ok(())
}

fn cleanup_artifact(path: Option<&str>, original_path: Option<&str>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if Some(path) == original_path {
        return Ok(());
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("Failed to remove best-of artifact {path}")),
    }
}
