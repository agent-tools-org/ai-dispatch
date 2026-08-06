// Output handling helpers for `aid run`.
// Exports fallback output extraction and JSONL cleanup utilities for run_prompt.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(in crate::cmd) fn output_file_instruction(output_path: Option<&str>, result_file: Option<&str>) -> Option<String> {
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
    /// The declared file was missing; captured output stood in for it.
    LogFallback { looks_like_report: bool },
}

pub(in crate::cmd) fn persist_result_file(
    task_id: &str,
    result_file: Option<&str>,
    base_dir: Option<&str>,
    log_path: &Path,
) -> Result<ResultDelivery> {
    let Some(result_file) = result_file else { return Ok(ResultDelivery::NotRequested); };
    let source = resolve_result_path(result_file, base_dir);
    if !source.exists() {
        return persist_result_from_log(task_id, log_path);
    }
    let dest = crate::paths::task_dir(task_id).join("result.md");
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

fn persist_result_from_log(task_id: &str, log_path: &Path) -> Result<ResultDelivery> {
    let Some(content) = extract_output_fallback_from_log(log_path) else {
        return Ok(ResultDelivery::LogFallback { looks_like_report: false });
    };
    let looks_like_report = crate::delivery_guard::looks_like_delivered_report(&content);
    let dest = crate::paths::task_dir(task_id).join("result.md");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, content)
        .with_context(|| format!("Failed to persist result file to {}", dest.display()))?;
    Ok(ResultDelivery::LogFallback { looks_like_report })
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

pub(in crate::cmd) fn fill_empty_output_from_log(log_path: &Path, output_path: Option<&Path>) -> Result<()> {
    let Some(output_path) = output_path else { return Ok(()) };
    let needs_fallback = match std::fs::metadata(output_path) {
        Ok(metadata) => metadata.len() == 0,
        Err(_) => true,
    };
    if !needs_fallback {
        return Ok(());
    }
    let Some(content) = extract_output_fallback_from_log(log_path) else { return Ok(()) };
    std::fs::write(output_path, content).with_context(|| {
        format!("Failed to write output fallback file {}", output_path.display())
    })
}

pub(in crate::cmd::run) fn extract_output_fallback_from_path(path: &Path) -> Option<String> {
    crate::cmd::show::extract_messages_from_log(path, true, None)
        .or_else(|| extract_raw_text_from_log(path))
        .filter(|content| !content.is_empty())
}

fn extract_output_fallback_from_log(log_path: &Path) -> Option<String> {
    extract_output_fallback_from_path(log_path)
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
    if json_lines * 2 <= non_empty_lines.len() {
        return Ok(());
    }
    let Some(cleaned) = crate::cmd::show::extract_messages_from_log(output_path, true, None) else {
        return Ok(());
    };
    std::fs::write(output_path, cleaned).with_context(|| {
        format!("Failed to rewrite cleaned output file {}", output_path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{NamedTempFile, tempdir};

    #[test]
    fn fill_empty_output_from_log_extracts_full_text_content() {
        let log = NamedTempFile::new().unwrap();
        let output = NamedTempFile::new().unwrap();
        std::fs::write(
            log.path(),
            concat!(
                "{\"type\":\"text\",\"content\":\"first chunk\"}\n",
                "{\"type\":\"text\",\"content\":\"second chunk\"}\n"
            ),
        )
        .unwrap();
        std::fs::write(output.path(), "").unwrap();

        fill_empty_output_from_log(log.path(), Some(output.path())).unwrap();

        assert_eq!(
            std::fs::read_to_string(output.path()).unwrap(),
            "second chunk"
        );
    }

    #[test]
    fn fill_empty_output_from_log_creates_missing_output_file() {
        let log = NamedTempFile::new().unwrap();
        let temp = tempdir().unwrap();
        let output_path = temp.path().join("missing-output.txt");
        std::fs::write(log.path(), "{\"type\":\"text\",\"content\":\"gemini output\"}\n").unwrap();

        fill_empty_output_from_log(log.path(), Some(output_path.as_path())).unwrap();

        assert_eq!(std::fs::read_to_string(output_path).unwrap(), "gemini output");
    }

    #[test]
    fn persist_result_file_resolves_relative_path_from_base_dir() {
        let temp = tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let work_dir = temp.path().join("work");
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::write(work_dir.join("result.md"), "structured result").unwrap();

        let log_path = crate::paths::log_path("t-result");
        let delivery = persist_result_file(
            "t-result",
            Some("result.md"),
            Some(work_dir.to_str().unwrap()),
            &log_path,
        )
        .unwrap();

        let saved = crate::paths::task_dir("t-result").join("result.md");
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "structured result");
        assert_eq!(delivery, ResultDelivery::File);
    }

    /// The failure that produced a "done" audit with a tool log for a report: the agent
    /// never wrote the result file and printed only what it was about to do.
    #[test]
    fn persist_result_file_flags_narration_only_log_fallback() {
        let temp = tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let log_path = crate::paths::log_path("t-narration-result");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let narration = "I will run `git diff main..HEAD` to get the list of changes made in the branch.\n\
I will read the generated diff file using the `view_file` tool to inspect the full set of changes.\n\
I will inspect the indexer snapshot file to answer the second question about determinism.\n";
        std::fs::write(&log_path, narration).unwrap();

        let delivery =
            persist_result_file("t-narration-result", Some("result.md"), Some("/missing"), &log_path)
                .unwrap();

        assert_eq!(delivery, ResultDelivery::LogFallback { looks_like_report: false });
    }

    #[test]
    fn persist_result_file_uses_log_when_source_missing() {
        let temp = tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let log_path = crate::paths::log_path("t-log-result");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(&log_path, "{\"type\":\"text\",\"content\":\"stdout report\"}\n").unwrap();

        persist_result_file("t-log-result", Some("result.md"), Some("/missing"), &log_path)
            .unwrap();

        let saved = crate::paths::task_dir("t-log-result").join("result.md");
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "stdout report");
    }

    #[test]
    fn persist_result_file_uses_raw_text_log_when_source_missing() {
        let temp = tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let log_path = crate::paths::log_path("t-raw-log-result");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(&log_path, "plain text report\nwith final findings").unwrap();

        let delivery =
            persist_result_file("t-raw-log-result", Some("result.md"), Some("/missing"), &log_path)
                .unwrap();

        let saved = crate::paths::task_dir("t-raw-log-result").join("result.md");
        assert_eq!(
            std::fs::read_to_string(saved).unwrap(),
            "plain text report\nwith final findings"
        );
        // Short, but not narration - it is still salvaged, just not blessed as a report.
        assert_eq!(delivery, ResultDelivery::LogFallback { looks_like_report: false });
    }

    #[test]
    fn persist_result_file_skips_empty_or_missing_log_when_source_missing() {
        let temp = tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let missing_log = crate::paths::log_path("t-missing-log-result");

        persist_result_file(
            "t-missing-log-result",
            Some("result.md"),
            Some("/missing"),
            &missing_log,
        )
        .unwrap();

        let missing_saved = crate::paths::task_dir("t-missing-log-result").join("result.md");
        assert!(!missing_saved.exists());

        let empty_log = crate::paths::log_path("t-empty-log-result");
        std::fs::create_dir_all(empty_log.parent().unwrap()).unwrap();
        std::fs::write(&empty_log, "").unwrap();

        persist_result_file(
            "t-empty-log-result",
            Some("result.md"),
            Some("/missing"),
            &empty_log,
        )
        .unwrap();

        let empty_saved = crate::paths::task_dir("t-empty-log-result").join("result.md");
        assert!(!empty_saved.exists());
    }
}
