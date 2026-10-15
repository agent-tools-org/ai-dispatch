// Detects unavailable-model failures from explicit error channels.
// Exports structured-event and stderr-line extractors for retry decisions.
// Deps: serde_json for parsing agent JSON error events.

/// True when `message` indicates the requested model id is not usable —
/// deprecated, renamed, unknown, or unsupported for this account. Distinct from
/// rate-limit/quota (transient) errors: this is fixed by choosing another model.
fn is_model_unavailable_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("model not found")
        || lower.contains("not supported model")
        || lower.contains("model_not_found")
        || lower.contains("unknown model")
        || lower.contains("invalid model")
        || lower.contains("no such model")
        || lower.contains("unsupported model")
        || (lower.contains("model") && lower.contains("is not supported"))
        || (lower.contains("model") && lower.contains("does not exist"))
        || (lower.contains("model") && lower.contains("is not available"))
        || (lower.contains("model") && lower.contains("has been deprecated"))
        || (lower.contains("model") && lower.contains("no longer available"))
}

/// Pull a model-unavailable message from a structured error event.
pub fn extract_model_unavailable_json_error(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    extract_from_json_error(trimmed)
}

/// Pull a model-unavailable message from one stderr line. Plain text is only
/// trusted on stderr and must use a captured provider error prefix.
pub fn extract_model_unavailable_stderr_line(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(message) = extract_from_json_error(trimmed) {
        return Some(message);
    }
    if is_explicit_plain_error(trimmed) && is_model_unavailable_error(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

fn is_explicit_plain_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.starts_with("error:")
        || lower.starts_with("api error:")
        || lower.starts_with("model not found")
        || lower.starts_with("not supported model")
        || lower.starts_with("unknown model")
        || lower.starts_with("unsupported model")
        || (lower.starts_with("the '") && lower.contains(" model "))
}

fn extract_from_json_error(json_str: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let explicit_error = value.get("error").is_some()
        || value.get("is_error").and_then(serde_json::Value::as_bool) == Some(true)
        || value.get("type").and_then(serde_json::Value::as_str) == Some("error");
    if !explicit_error {
        return None;
    }
    // Walk common nesting shapes: {message}, {error:{message}}, {error:{data:{message}}}.
    for candidate in [
        value.get("message"),
        value.get("error").and_then(|e| e.get("message")),
        value.get("error").and_then(|e| e.get("data")).and_then(|d| d.get("message")),
    ] {
        if let Some(msg) = candidate.and_then(serde_json::Value::as_str) {
            let msg = msg.trim();
            if !msg.is_empty() && is_model_unavailable_error(msg) {
                return Some(msg.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_real_agent_failures() {
        // opencode
        assert!(is_model_unavailable_error("Model not found: glm-4.7/."));
        // mimo
        assert!(is_model_unavailable_error("Not supported model mimo-v2.5-pro-ultraspeed: Param Incorrect"));
        // codex
        assert!(is_model_unavailable_error(
            "The 'gpt-4.1-nano' model is not supported when using Codex with a ChatGPT account."
        ));
    }

    #[test]
    fn ignores_unrelated_and_transient_errors() {
        assert!(!is_model_unavailable_error("rate limit exceeded"));
        assert!(!is_model_unavailable_error("Insufficient balance. Manage your billing"));
        assert!(!is_model_unavailable_error("Authentication failed. Please log in"));
        assert!(!is_model_unavailable_error("connection reset by peer"));
    }

    #[test]
    fn extracts_from_json_error_events() {
        let opencode = r#"{"type":"error","error":{"name":"UnknownError","data":{"message":"Model not found: glm-4.7/."}}}"#;
        assert_eq!(
            extract_model_unavailable_json_error(opencode).as_deref(),
            Some("Model not found: glm-4.7/.")
        );
        let codex = r#"{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-4.1-nano' model is not supported when using Codex with a ChatGPT account."}}"#;
        assert!(extract_model_unavailable_json_error(codex).is_some());
    }

    #[test]
    fn ignores_json_rate_limit_events() {
        let rl = r#"{"type":"error","error":{"message":"429 rate limit exceeded"}}"#;
        assert!(extract_model_unavailable_json_error(rl).is_none());
    }

    #[test]
    fn extracts_from_explicit_stderr_line_without_a_length_rule() {
        let detail = "x".repeat(600);
        let line = format!("Error: unknown model 'foo': {detail}");
        assert_eq!(
            extract_model_unavailable_stderr_line(&line).as_deref(),
            Some(line.as_str())
        );
    }

    #[test]
    fn ignores_plain_prose_and_non_error_json() {
        assert!(extract_model_unavailable_stderr_line(
            "The report says an unknown model would be rejected"
        )
        .is_none());
        let message = r#"{"type":"agent_message","message":"Error: unknown model 'foo'"}"#;
        assert!(extract_model_unavailable_json_error(message).is_none());
    }
}
