// Artifact fallback helpers for `aid show` diff rendering.
// Exports: diff_artifact_fallback for missing worktree cases.
// Deps: paths, show_output_messages::read_task_output, Task.
use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::types::Task;

use super::show_output_messages::read_task_output;

pub(super) fn diff_artifact_fallback(task: &Task, task_id: &str) -> Result<Option<String>> {
    if let Ok(task_output) = read_task_output(task) {
        return Ok(Some(format_artifact_block("Output", &task_output)));
    }
    let log_path = task
        .log_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::log_path(task_id));
    if let Some(edits) = extract_edits_from_log(&log_path) {
        return Ok(Some(edits));
    }
    if let Ok(log) = std::fs::read_to_string(&log_path) {
        return Ok(Some(format_artifact_block("Output", &log)));
    }
    Ok(None)
}

fn extract_edits_from_log(log_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(log_path).ok()?;
    let mut edits: Vec<(String, String, String, String)> = Vec::new();
    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some("tool_use") = obj.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(part) = obj.get("part") else { continue };
        let Some(tool) = part.get("tool").and_then(|v| v.as_str()) else {
            continue;
        };
        if !matches!(tool, "edit" | "write" | "apply_diff" | "replace") {
            continue;
        }
        let Some(input) = part.get("state").and_then(|s| s.get("input")) else {
            continue;
        };
        let file_path = input
            .get("filePath")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)")
            .to_string();
        let old = input
            .get("oldString")
            .or_else(|| input.get("old_string"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new = input
            .get("newString")
            .or_else(|| input.get("new_string"))
            .or_else(|| input.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        edits.push((file_path, tool.to_string(), old, new));
    }
    if edits.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str("\n--- File Changes (from agent log) ---\n");
    let mut files: Vec<&str> = edits.iter().map(|(file, ..)| file.as_str()).collect();
    files.dedup();
    out.push_str(&format!(
        "  {} edit(s) across {} file(s):\n",
        edits.len(),
        files.len()
    ));
    for file in &files {
        let short = shorten_path(file, 3);
        out.push_str(&format!("    {short}\n"));
    }
    for (file, tool, old, new) in &edits {
        let short = shorten_path(file, 2);
        out.push_str(&format!("\n  [{tool}] {short}\n"));
        push_preview_lines(&mut out, old, "-");
        push_preview_lines(&mut out, new, "+");
    }
    Some(out)
}

fn shorten_path(path: &str, components: usize) -> String {
    path.rsplit('/')
        .take(components)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("/")
}

fn push_preview_lines(out: &mut String, content: &str, prefix: &str) {
    if content.is_empty() {
        return;
    }
    let line_count = content.lines().count();
    for line in content.lines().take(10) {
        out.push_str(&format!("  {prefix} {line}\n"));
    }
    if line_count > 10 {
        out.push_str(&format!("  ... ({} more lines)\n", line_count - 10));
    }
}

fn format_artifact_block(title: &str, content: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("\n--- {title} ---\n"));
    out.push_str(content);
    out
}
