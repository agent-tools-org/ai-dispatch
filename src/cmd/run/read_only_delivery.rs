// Stage native Codex read-only artifacts inside the granted task directory.
// Exports agent_paths and publish for prompt, worker and lifecycle delivery.
// Deps: RunArgs, task paths and trusted filesystem copies after agent exit.

use super::RunArgs;
use crate::types::AgentKind;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

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

pub(crate) fn publish(args: &RunArgs, kind: AgentKind, task_id: &str, dir: Option<&str>) -> Result<()> {
    if !enabled(args, kind) {
        return Ok(());
    }
    let (output, result) = agent_paths(args, kind, task_id);
    for (source, destination) in [
        (output.as_deref(), args.output.as_deref()),
        (result.as_deref(), args.result_file.as_deref()),
    ] {
        let (Some(source), Some(destination)) = (source, destination) else {
            continue;
        };
        if !std::fs::read_to_string(source).is_ok_and(|content| !content.trim().is_empty()) {
            continue;
        }
        let destination = resolve(destination, dir);
        if Path::new(source) == destination {
            continue;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, &destination)
            .with_context(|| format!("Cannot deliver read-only artifact to '{}'", destination.display()))?;
    }
    Ok(())
}

fn resolve(path: &str, dir: Option<&str>) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(dir.unwrap_or(".")).join(path)
    }
}
