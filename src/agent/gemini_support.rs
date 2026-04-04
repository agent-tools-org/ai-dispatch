// Gemini adapter helpers for CLI args and event classification.
// Exports: include-directory, milestone, rate-limit, and truncation helpers.
// Deps: serde_json for event fields and std::path for workspace checks.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub(super) fn gemini_include_directories(run_dir: Option<&str>, context_files: &[String]) -> Vec<String> {
    let run_dir = resolve_run_dir(run_dir);
    let mut directories = BTreeSet::new();
    for file in context_files {
        let parent = Path::new(file).parent().unwrap_or_else(|| Path::new("."));
        if should_include_directory(parent, run_dir.as_deref()) {
            directories.insert(parent.to_string_lossy().into_owned());
        }
    }
    directories.into_iter().collect()
}

pub(super) fn is_skill_or_hook_event(event_type: &str) -> bool {
    event_type.contains("skill") || event_type.contains("hook")
}

pub(super) fn milestone_detail(event_type: &str, value: &Value) -> String {
    let label = if event_type.contains("skill") { "skill" } else { "hook" };
    if let Some(message) = [
        value.get("message"),
        value.get("detail"),
        value.get("summary"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    {
        return format!("{label}: {}", truncate(message, 80));
    }

    let name = [
        value.get("name"),
        value.get("skill"),
        value.get("hook"),
        value.pointer("/metadata/name"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str);
    let status = [
        value.get("status"),
        value.get("phase"),
        value.get("state"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str);

    match (name, status) {
        (Some(name), Some(status)) => format!("{label} {name}: {status}"),
        (Some(name), None) => format!("{label} {name}"),
        (None, Some(status)) => format!("{label}: {status}"),
        (None, None) => label.to_string(),
    }
}

pub(super) fn is_gemini_rate_limit_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    crate::rate_limit::is_rate_limit_error(message)
        || lower.contains("resourceexhausted")
        || lower.contains("resource exhausted")
        || lower.contains("rate_limit_exceeded")
}

pub(super) fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let safe = text.floor_char_boundary(max_len.saturating_sub(3));
        format!("{}...", &text[..safe])
    }
}

pub(super) fn extract_tool_name(value: &Value) -> Option<&str> {
    value.get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.pointer("/functionCall/name").and_then(Value::as_str))
        .or_else(|| value.get("function_call").and_then(Value::as_str))
        .or_else(|| value.get("function_call").and_then(|v| v.get("name")).and_then(Value::as_str))
        .or_else(|| value.get("toolName").and_then(Value::as_str))
        .or_else(|| value.get("tool").and_then(Value::as_str))
        .or_else(|| value.get("tool").and_then(|v| v.get("name")).and_then(Value::as_str))
}

pub(super) fn extract_tool_arguments(value: &Value) -> Option<String> {
    [
        value.get("arguments"),
        value.pointer("/functionCall/args"),
        value.get("parameters"),
        value.get("input"),
    ]
    .into_iter()
    .flatten()
    .find_map(stringify_value)
}

pub(super) fn classify_tool_result(name: &str, output: &str) -> (crate::types::EventKind, String) {
    let lower_output = output.to_lowercase();
    let lower_name = name.to_lowercase();
    if lower_output.contains("error") || lower_output.contains("failed") || lower_output.contains("failure") {
        (crate::types::EventKind::Error, format!("{}: {}", name, truncate(output, 80)))
    } else if lower_name.contains("test") || lower_output.contains("test") || lower_output.contains("passed") || lower_output.contains("failed") {
        (crate::types::EventKind::Test, format!("{}: {}", name, truncate(output, 80)))
    } else if lower_name.contains("build") || lower_name.contains("compile") || lower_output.contains("compiled") || lower_output.contains("built") {
        (crate::types::EventKind::Build, format!("{}: {}", name, truncate(output, 80)))
    } else {
        (crate::types::EventKind::ToolCall, format!("{}: {}", name, truncate(output, 80)))
    }
}

pub(super) fn extract_completion_stats(value: &Value) -> (Option<i64>, Option<String>) {
    let stats = match value.get("stats") {
        Some(stats) => stats,
        None => return (None, None),
    };
    if let Some(total) = stats.get("total_tokens").and_then(Value::as_i64) {
        let model = stats.get("models").and_then(Value::as_object).and_then(|obj| obj.keys().next().cloned());
        return (Some(total), model);
    }
    if let Some(models) = stats.get("models").and_then(Value::as_array) {
        let first = match models.first() {
            Some(first) => first,
            None => return (None, None),
        };
        let tokens = first.pointer("/tokens/total").and_then(Value::as_i64);
        let model = first.get("model").and_then(Value::as_str).map(ToOwned::to_owned);
        return (tokens, model);
    }
    (None, None)
}

pub(super) fn extract_tokens(value: &Value) -> Option<i64> {
    if let Some(total) = value.pointer("/stats/total_tokens").and_then(Value::as_i64) {
        return Some(total);
    }
    if let Some(models) = value.pointer("/stats/models").and_then(Value::as_array) {
        let total: i64 = models.iter().filter_map(|model| model.pointer("/tokens/total").and_then(Value::as_i64)).sum();
        if total > 0 {
            return Some(total);
        }
    }
    if let Some(models) = value.pointer("/stats/models").and_then(Value::as_object) {
        let total: i64 = models.values().filter_map(|model| model.get("total_tokens").and_then(Value::as_i64)).sum();
        if total > 0 {
            return Some(total);
        }
    }
    value.pointer("/usageMetadata/totalTokenCount").and_then(Value::as_i64)
}

pub(super) fn extract_error_detail(value: &Value) -> Option<String> {
    [
        value.get("message"),
        value.pointer("/error/message"),
        value.pointer("/error/status"),
        value.pointer("/error/details/0/reason"),
        value.pointer("/error/details/0/message"),
    ]
    .into_iter()
    .flatten()
    .find_map(stringify_value)
}

pub(super) fn extract_model(value: &Value) -> Option<String> {
    for path in ["/modelVersion", "/model", "/stats/models/0/model"] {
        if let Some(model) = value.pointer(path).and_then(Value::as_str) {
            return Some(model.to_string());
        }
    }
    value.pointer("/stats/models").and_then(Value::as_object).and_then(|obj| obj.keys().next().cloned())
}

fn stringify_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn resolve_run_dir(run_dir: Option<&str>) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let dir = run_dir.map(PathBuf::from).unwrap_or(cwd);
    Some(if dir.is_absolute() { dir } else { std::env::current_dir().ok()?.join(dir) })
}

fn should_include_directory(dir: &Path, run_dir: Option<&Path>) -> bool {
    if dir == Path::new(".") || dir.as_os_str().is_empty() {
        return false;
    }
    if dir.is_relative() {
        return matches!(dir.components().next(), Some(Component::ParentDir));
    }
    match run_dir {
        Some(base) => !dir.starts_with(base),
        None => true,
    }
}
