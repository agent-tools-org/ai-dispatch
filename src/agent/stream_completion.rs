// Shared streaming completion status from real CLI JSONL envelopes.
// Used by adapters' parse_completion and by streaming finalize paths.
// Deps: serde_json, TaskStatus / CompletionInfo.

use serde_json::Value;

use crate::types::{CompletionInfo, TaskStatus};

/// Claude / Cursor / Qwen stream-json terminal `result` is a failure.
/// Keyed to real envelopes: `is_error:true` or non-`success` subtype.
pub(crate) fn result_envelope_failed(v: &Value) -> bool {
    if v.get("type").and_then(|t| t.as_str()) != Some("result") {
        return false;
    }
    if v.get("is_error").and_then(|b| b.as_bool()) == Some(true) {
        return true;
    }
    matches!(
        v.get("subtype").and_then(|s| s.as_str()),
        Some(sub) if sub != "success"
    )
}

/// Any JSONL line with top-level `type == "error"` (OpenCode / Gemini / etc.).
pub(crate) fn jsonl_has_error_type(output: &str) -> bool {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("error") {
            return true;
        }
    }
    false
}

/// Status from Claude/Cursor-style result envelopes plus any `type:error` line.
pub(crate) fn status_from_result_jsonl(output: &str) -> CompletionInfo {
    let mut failed = jsonl_has_error_type(output);
    if !failed {
        for line in output.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if result_envelope_failed(&v) {
                failed = true;
                break;
            }
        }
    }
    CompletionInfo {
        tokens: None,
        status: if failed {
            TaskStatus::Failed
        } else {
            TaskStatus::Done
        },
        model: None,
        cost_usd: None,
        exit_code: None,
    }
}

/// Status from `type == "error"` only — no plaintext heuristics.
pub(crate) fn status_from_error_type_jsonl(output: &str) -> CompletionInfo {
    CompletionInfo {
        tokens: None,
        status: if jsonl_has_error_type(output) {
            TaskStatus::Failed
        } else {
            TaskStatus::Done
        },
        model: None,
        cost_usd: None,
        exit_code: None,
    }
}

/// Merge parse_completion into streaming info: Failed wins; fill missing fields.
pub(crate) fn merge_parsed_completion(info: &mut CompletionInfo, parsed: CompletionInfo) {
    if parsed.status == TaskStatus::Failed {
        info.status = TaskStatus::Failed;
    }
    if info.tokens.is_none() {
        info.tokens = parsed.tokens;
    }
    if info.model.is_none() {
        info.model = parsed.model;
    }
    if info.cost_usd.is_none() {
        info.cost_usd = parsed.cost_usd;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_is_error_true_fails() {
        let v: Value = serde_json::from_str(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
        )
        .unwrap();
        assert!(result_envelope_failed(&v));
    }

    #[test]
    fn result_success_is_error_false_ok() {
        let v: Value = serde_json::from_str(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#,
        )
        .unwrap();
        assert!(!result_envelope_failed(&v));
    }

    #[test]
    fn nested_opencode_error_type_fails() {
        let out = r#"{"type":"error","error":{"name":"UnknownError","data":{"message":"x"}}}"#;
        assert_eq!(
            status_from_result_jsonl(out).status,
            TaskStatus::Failed
        );
    }
}
