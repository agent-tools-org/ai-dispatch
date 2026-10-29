// Durable verification execution records independent of worker stdout.
// Stores command, local checkout identity, process status, and bounded output in task events.
use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::store::Store;
use crate::task::verify::VerifyResult;
use crate::types::{EventKind, TaskEvent, TaskId};

fn head(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn record(
    store: &Store,
    task_id: &TaskId,
    dir: &Path,
    requested: &str,
    head_before: Option<&str>,
    result: Option<&VerifyResult>,
) -> Result<()> {
    let command = result
        .map(|result| result.command.as_str())
        .unwrap_or(requested);
    let output = result.and_then(|result| super::verify_output_excerpt(&result.output));
    let detail = match result {
        Some(result) => format!(
            "Verification finished: {command}; exit={:?}; success={}{}",
            result.exit_code,
            result.success,
            output
                .as_deref()
                .map(|text| format!("\nOutput: {text}"))
                .unwrap_or_default()
        ),
        None => format!("Verification starting: {command} in {}", dir.display()),
    };
    store.insert_event(&TaskEvent::new(task_id.clone(), EventKind::Milestone, detail)
        .with_metadata(Some(serde_json::json!({
            "verification": if result.is_some() { "finished" } else { "started" },
            "requested_command": requested,
            "command": command,
            "directory": dir,
            "local_head_before": head_before,
            "local_head_after": head(dir),
            "remote_tested_head": null,
            "exit_code": result.and_then(|result| result.exit_code),
            "success": result.map(|result| result.success),
            "timed_out": result.map(|result| result.timed_out),
            "output_excerpt": output,
        }))))?;
    Ok(())
}

pub(super) fn run(
    store: &Store,
    task_id: &TaskId,
    path: &Path,
    requested: &str,
    command: Option<&str>,
    target: Option<&str>,
    container: Option<&str>,
) -> Result<VerifyResult> {
    let head_before = head(path);
    record(
        store,
        task_id,
        path,
        requested,
        head_before.as_deref(),
        None,
    )?;
    let result = crate::runtime::remote_build::verify(
        store,
        task_id.as_str(),
        path,
        command,
        target,
        container,
    )?;
    record(
        store,
        task_id,
        path,
        requested,
        head_before.as_deref(),
        Some(&result),
    )?;
    Ok(result)
}
