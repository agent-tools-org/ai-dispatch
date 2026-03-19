// Output handling helpers for `aid run`.
// Exports fallback output extraction and JSONL cleanup utilities for run_prompt.
use anyhow::{Context, Result};
use std::path::Path;

pub(in crate::cmd) fn output_file_instruction() -> String {
    "IMPORTANT: Your final response will be saved to a file. Write ONLY the requested deliverable content in your final response. Do NOT include planning, reasoning, chain-of-thought, or meta-commentary. The file should contain only the finished work product.".to_string()
}

pub(in crate::cmd) fn fill_empty_output_from_log(log_path: &Path, output_path: Option<&Path>) -> Result<()> {
    let Some(output_path) = output_path else { return Ok(()) };
    let needs_fallback = match std::fs::metadata(output_path) {
        Ok(metadata) => metadata.len() == 0,
        Err(_) => true,
    };
    if !needs_fallback {
        return Ok(());
    }
    let content = crate::cmd::show::extract_messages_from_log(log_path, false)
        .or_else(|| extract_raw_text_from_log(log_path));
    let Some(content) = content else { return Ok(()) };
    if content.is_empty() {
        return Ok(());
    }
    std::fs::write(output_path, content).with_context(|| {
        format!("Failed to write output fallback file {}", output_path.display())
    })
}

fn extract_raw_text_from_log(log_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(log_path).ok()?;
    let raw_lines: Vec<&str> = content
        .lines()
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
    if json_lines * 2 <= non_empty_lines.len() {
        return Ok(());
    }
    let Some(cleaned) = crate::cmd::show::extract_messages_from_log(output_path, true) else {
        return Ok(());
    };
    std::fs::write(output_path, cleaned).with_context(|| {
        format!("Failed to rewrite cleaned output file {}", output_path.display())
    })
}
