// Provider capacity evidence and model-scoped holds using existing route markers.
// Exports classification, captured-error lookup, availability and advice notes.
// Deps: quota channels, rate_limit markers and route availability.

use crate::quota_channel::Channel;
use crate::{paths, rate_limit, route_availability as routes, types::*};

pub(crate) const HOLD_MINUTES: i64 = 2;
const REFUSAL: &str = "selected model is at capacity. please try a different model.";
const RECONNECT_FAILURE: &str = "provider capacity: exhausted 503 reconnects";

pub(crate) fn is_capacity(message: &str) -> bool {
    let lower = message.trim().to_ascii_lowercase();
    let lower = lower.strip_prefix("error: ").unwrap_or(&lower);
    lower == REFUSAL || lower.starts_with(RECONNECT_FAILURE)
}

pub(crate) fn classify(raw: &str, _agent: AgentKind, _channel: Channel) -> Option<String> {
    let mut exhausted = false;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Some(message) = final_error(line) else {
            exhausted = false;
            continue;
        };
        if is_capacity(&message) {
            return Some(message);
        }
        let lower = message.trim().to_ascii_lowercase();
        if exhausted && !lower.starts_with("reconnecting...") {
            return Some(format!("{RECONNECT_FAILURE}: {message}"));
        }
        exhausted = lower.starts_with("reconnecting...")
            && lower.contains("5/5")
            && lower.contains("503 service unavailable");
    }
    None
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

pub(crate) fn read(task_id: &TaskId, agent: AgentKind) -> Option<String> {
    let stderr = std::fs::read_to_string(paths::stderr_path(task_id.as_str())).unwrap_or_default();
    let log = std::fs::read_to_string(paths::log_path(task_id.as_str())).unwrap_or_default();
    classify(&stderr, agent, Channel::CliStderr)
        .or_else(|| classify(&log, agent, Channel::CliStream))
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
