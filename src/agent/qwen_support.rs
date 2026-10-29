// Qwen stream event text, tool, and milestone helpers.
// Deps: serde_json values and shared event-detail bounds.

use crate::ui::format::truncate;
use serde_json::Value;

pub(super) fn is_skill_or_hook_event(event_type: &str) -> bool {
    event_type.contains("skill") || event_type.contains("hook")
}

pub(super) fn milestone_detail(event_type: &str, value: &Value) -> String {
    let label = if event_type.contains("skill") {
        "skill"
    } else {
        "hook"
    };
    if let Some(message) = [
        value.get("message"),
        value.get("detail"),
        value.get("summary"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    {
        return format!(
            "{label}: {}",
            truncate(message, super::truncate::EVENT_DETAIL_MAX)
        );
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
    let status = [value.get("status"), value.get("phase"), value.get("state")]
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

pub(super) fn extract_tool_name(value: &Value) -> Option<&str> {
    value
        .get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.pointer("/functionCall/name").and_then(Value::as_str))
        .or_else(|| value.get("function_call").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("function_call")
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str)
        })
        .or_else(|| value.get("toolName").and_then(Value::as_str))
        .or_else(|| value.get("tool").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("tool")
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str)
        })
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
    use crate::types::EventKind;
    let lower_output = output.to_lowercase();
    let lower_name = name.to_lowercase();
    let kind = if lower_name.contains("test")
        || lower_output.contains("test")
        || lower_output.contains("passed")
    {
        EventKind::Test
    } else if lower_output.contains("error")
        || lower_output.contains("failed")
        || lower_output.contains("failure")
    {
        EventKind::Error
    } else if lower_name.contains("build")
        || lower_name.contains("compile")
        || lower_output.contains("compiled")
        || lower_output.contains("built")
    {
        EventKind::Build
    } else {
        EventKind::ToolCall
    };
    (
        kind,
        format!(
            "{}: {}",
            name,
            truncate(output, super::truncate::EVENT_DETAIL_MAX)
        ),
    )
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

fn stringify_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream_text::extract_text_payload;

    #[test]
    fn qwen_helpers_preserve_tool_and_text_shapes() {
        let value = serde_json::json!({"functionCall":{"name":"inspect", "args":{"path":"a.rs"}}});
        assert_eq!(extract_tool_name(&value), Some("inspect"));
        assert_eq!(
            extract_tool_arguments(&value).as_deref(),
            Some("{\"path\":\"a.rs\"}")
        );
        assert_eq!(
            extract_text_payload(Some(&serde_json::json!([{"text":"a"}, {"text":"b"}]))),
            Some("ab".into())
        );
        assert_eq!(
            extract_error_detail(&serde_json::json!({"error":{"message":"failed"}})),
            Some("failed".into())
        );
        assert_eq!(truncate("ééééé", 7), "éé...");
        assert!(is_skill_or_hook_event("hook_start"));
        assert_eq!(
            milestone_detail("skill_start", &serde_json::json!({"name":"review"})),
            "skill review"
        );
        for (name, output, kind) in [
            ("test", "passed", crate::types::EventKind::Test),
            ("read", "failed", crate::types::EventKind::Error),
            ("build", "built", crate::types::EventKind::Build),
            ("read", "ok", crate::types::EventKind::ToolCall),
        ] {
            assert_eq!(
                classify_tool_result(name, output),
                (kind, format!("{name}: {output}"))
            );
        }
    }
}
