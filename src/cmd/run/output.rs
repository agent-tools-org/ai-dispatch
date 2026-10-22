// Output handling helpers for `aid run`.
// Exports fallback output extraction and JSONL cleanup utilities for run_prompt.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
pub(in crate::cmd) fn output_file_instruction(
    output_path: Option<&str>,
    result_file: Option<&str>,
) -> Option<String> {
    let mut sections = Vec::new();
    if output_path.is_some() {
        sections.push("IMPORTANT: Your final response will be saved to a file. Write ONLY the requested deliverable content in your final response. Do NOT include planning, reasoning, chain-of-thought, or meta-commentary. The file should contain only the finished work product.".to_string());
    }
    if let Some(path) = result_file {
        sections.push(format!("<aid-result-file>{path}</aid-result-file>\nWrite your structured findings/results to the file specified above. This file will be preserved as the task's official result."));
    }
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

/// How the task's `result.md` was obtained. The log fallback keeps evidence around when
/// an agent ignores the result-file instruction, but it must never be mistaken for a
/// report the agent actually wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::cmd) enum ResultDelivery {
    NotRequested,
    /// The agent wrote the declared result file.
    File,
    /// The declared file was missing; a captured-output fallback may have been saved.
    MissingFile {
        fallback_saved: bool,
    },
}

pub(in crate::cmd) fn persist_result_file(
    task_id: &str,
    result_file: Option<&str>,
    base_dir: Option<&str>,
    log_path: &Path,
    agent_name: Option<&str>,
) -> Result<ResultDelivery> {
    let Some(result_file) = result_file else {
        return Ok(ResultDelivery::NotRequested);
    };
    let source = resolve_result_path(result_file, base_dir);
    if !file_has_content(&source) {
        return persist_result_from_log(task_id, log_path, agent_name);
    }
    let dest = crate::support::paths::task_dir(task_id).join("result.md");
    if source.as_path() == dest.as_path() {
        return Ok(ResultDelivery::File);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, &dest)
        .with_context(|| format!("Failed to persist result file to {}", dest.display()))?;
    Ok(ResultDelivery::File)
}

fn persist_result_from_log(
    task_id: &str,
    log_path: &Path,
    agent_name: Option<&str>,
) -> Result<ResultDelivery> {
    let Some(content) = extract_output_fallback_from_log(log_path, agent_name) else {
        return Ok(ResultDelivery::MissingFile {
            fallback_saved: false,
        });
    };
    let dest = crate::support::paths::task_dir(task_id).join("result.md");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, content)
        .with_context(|| format!("Failed to persist result file to {}", dest.display()))?;
    Ok(ResultDelivery::MissingFile {
        fallback_saved: true,
    })
}

fn file_has_content(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|content| !content.trim().is_empty())
}

fn resolve_result_path(result_file: &str, base_dir: Option<&str>) -> PathBuf {
    let path = Path::new(result_file);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(base_dir) = base_dir {
        return Path::new(base_dir).join(path);
    }
    path.to_path_buf()
}

pub(in crate::cmd) fn fill_empty_output_from_log(
    log_path: &Path,
    output_path: Option<&Path>,
    agent_name: Option<&str>,
) -> Result<()> {
    let Some(output_path) = output_path else {
        return Ok(());
    };
    let needs_fallback = match std::fs::metadata(output_path) {
        Ok(metadata) => metadata.len() == 0,
        Err(_) => true,
    };
    if !needs_fallback {
        return Ok(());
    }
    let Some(content) = extract_output_fallback_from_log(log_path, agent_name) else {
        return Ok(());
    };
    std::fs::write(output_path, content).with_context(|| {
        format!(
            "Failed to write output fallback file {}",
            output_path.display()
        )
    })
}

pub(in crate::cmd::run) fn extract_output_fallback_from_path(
    path: &Path,
    agent_name: Option<&str>,
) -> Option<String> {
    crate::cmd::show::extract_messages_from_log(path, true, agent_name)
        .or_else(|| extract_raw_text_from_log(path))
        .filter(|content| !content.is_empty())
}

fn extract_output_fallback_from_log(log_path: &Path, agent_name: Option<&str>) -> Option<String> {
    extract_output_fallback_from_path(log_path, agent_name)
}

fn extract_raw_text_from_log(log_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(log_path).ok()?;
    let raw_lines: Vec<&str> = content
        .lines()
        .filter(|line| !crate::cmd::show::is_non_output_line(line))
        .filter(|line| serde_json::from_str::<serde_json::Value>(line).is_err())
        .collect();
    raw_lines
        .iter()
        .any(|line| !line.trim().is_empty())
        .then(|| raw_lines.join("\n"))
}

pub(in crate::cmd) fn clean_output_if_jsonl(output_path: &Path) -> Result<()> {
    let content = match std::fs::read_to_string(output_path) {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };
    let non_empty_lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_empty_lines.is_empty() {
        return Ok(());
    }
    let json_lines = non_empty_lines
        .iter()
        .filter(|line| {
            line.starts_with('{')
                && matches!(
                    serde_json::from_str::<serde_json::Value>(line),
                    Ok(serde_json::Value::Object(_))
                )
        })
        .count();
    if json_lines != non_empty_lines.len() {
        return Ok(());
    }
    let Some(cleaned) = crate::cmd::show::extract_messages_from_log(output_path, true, None) else {
        return Ok(());
    };
    std::fs::write(output_path, cleaned).with_context(|| {
        format!(
            "Failed to rewrite cleaned output file {}",
            output_path.display()
        )
    })
}

#[cfg(test)]
#[path = "output/tests.rs"]
mod tests;
