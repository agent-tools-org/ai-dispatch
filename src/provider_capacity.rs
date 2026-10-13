// Provider capacity evidence and model-scoped holds using existing route markers.
// Exports classification, captured-error lookup, availability and advice notes.
// Deps: quota channels, rate_limit markers and route availability.

use crate::quota_channel::{Channel, provider_attributable};
use crate::{paths, rate_limit, route_availability as routes, types::*};

pub(crate) const HOLD_MINUTES: i64 = 2;
const REFUSAL: &str = "selected model is at capacity. please try a different model.";
const RECONNECT_FAILURE: &str = "provider capacity: exhausted 503 reconnects";

pub(crate) fn is_capacity(message: &str) -> bool {
    let lower = message.trim().to_ascii_lowercase();
    let lower = lower.strip_prefix("error: ").unwrap_or(&lower);
    lower == REFUSAL || lower.starts_with(RECONNECT_FAILURE)
}

pub(crate) fn classify(raw: &str, agent: AgentKind, channel: Channel) -> Option<String> {
    let kept = provider_attributable(raw, agent, channel);
    let mut diagnostic = kept.cli_diagnostic;
    if let Some(message) = kept.unsplit.lines().find(|line| is_capacity(line)) {
        return Some(message.to_string());
    }
    if let Some(message) = reconnect_tail(&kept.unsplit) {
        return Some(message);
    }
    if channel == Channel::CliStream {
        for line in raw.lines() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
                && value.get("type").and_then(serde_json::Value::as_str) == Some("item.completed")
                && value
                    .pointer("/item/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("error")
                && let Some(message) = value
                    .pointer("/item/message")
                    .and_then(serde_json::Value::as_str)
            {
                diagnostic.push_str(message);
                diagnostic.push('\n');
            }
        }
    }
    if let Some(message) = diagnostic.lines().find(|line| is_capacity(line)) {
        return Some(message.to_string());
    }
    if channel == Channel::CliStderr {
        return reconnect_tail(&diagnostic);
    }
    let mut lines = raw.lines().rev().filter(|line| !line.trim().is_empty());
    let message = final_error(lines.next()?)?;
    let previous = lines.next()?;
    let parsed: Option<serde_json::Value> = serde_json::from_str(previous).ok();
    let exhausted = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(previous);
    reconnect_tail(&format!("{exhausted}\nError: {message}"))
}

fn final_error(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if !matches!(value.get("type")?.as_str()?, "error" | "turn.failed") {
        return None;
    }
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))?
        .as_str()
        .map(str::to_string)
}

fn reconnect_tail(content: &str) -> Option<String> {
    let mut exhausted = false;
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("reconnecting...") {
            exhausted = lower.contains("5/5") && lower.contains("503 service unavailable");
        } else if exhausted && lower.starts_with("error:") {
            return Some(format!("{RECONNECT_FAILURE}: {line}"));
        } else {
            exhausted = false;
        }
    }
    None
}

pub(crate) fn read(task_id: &TaskId, agent: AgentKind) -> Option<String> {
    let stderr = std::fs::read_to_string(paths::stderr_path(task_id.as_str())).unwrap_or_default();
    let log = std::fs::read_to_string(paths::log_path(task_id.as_str())).unwrap_or_default();
    classify(&stderr, agent, Channel::CliStderr)
        .or_else(|| classify(&log, agent, Channel::CliStream))
        .or_else(|| {
            let exhausted = stderr.lines().rev().find(|line| !line.trim().is_empty())?;
            let message = final_error(log.lines().last()?)?;
            reconnect_tail(&format!("{exhausted}\nError: {message}"))
        })
}

pub(crate) fn terminal_label(
    task_id: &TaskId,
    agent: AgentKind,
    status: TaskStatus,
) -> &'static str {
    if status == TaskStatus::Failed && read(task_id, agent).is_some() {
        "provider capacity"
    } else {
        status.label()
    }
}

pub(crate) fn group(model: Option<&str>) -> String {
    let encoded: String = model
        .unwrap_or("(default)")
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("capacity-{encoded}")
}

pub fn availability_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> routes::RouteAvailability {
    let capacity = routes::availability_for_group(agent, custom_name, &group(model));
    if capacity.status == routes::RouteStatus::Held {
        return capacity;
    }
    routes::quota_availability_for_model(agent, custom_name, model)
}

pub(crate) fn notes(agent: &AgentKind, custom_name: Option<&str>) -> Vec<String> {
    let prefix = format!(
        "rate-limit-{}--capacity-",
        rate_limit::marker_slug(agent, custom_name)
    );
    let Ok(entries) = std::fs::read_dir(paths::aid_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let encoded = name.strip_prefix(&prefix)?;
            let bytes: Option<Vec<u8>> = (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(encoded.get(i..i + 2)?, 16).ok())
                .collect();
            let model = String::from_utf8(bytes?).ok()?;
            let avail =
                routes::availability_for_group(agent, custom_name, &format!("capacity-{encoded}"));
            (avail.status == routes::RouteStatus::Held).then(|| format!("{model}: {}", avail.why))
        })
        .collect()
}

#[cfg(test)]
#[path = "provider_capacity_tests.rs"]
mod tests;
