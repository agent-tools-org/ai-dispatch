// MCP `classify` tool: the same request path and JSON as `aid classify`.
// Exports: classify_tool. Errors return as MCP tool errors with the CLI's messages.
// Deps: typesafe::classify, serde_json, tokio blocking pool.

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::typesafe::classify::{ClassifyError, ClassifyRequest, ClassifyState, DEFAULT_MODEL, DEFAULT_TIMEOUT_SECS, ErrorKind, classify};

/// The answer JSON, or `{"error": message}` for the caller to mark as a tool error.
pub(crate) async fn classify_tool(arguments: Value) -> Result<Value> {
    let request = match request_from_arguments(arguments) {
        Ok(request) => request,
        Err(err) => return Ok(json!({ "error": err.message })),
    };
    let answer = tokio::task::spawn_blocking(move || classify(&request)).await?;
    Ok(answer.unwrap_or_else(|err| json!({ "error": err.message })))
}

fn request_from_arguments(arguments: Value) -> Result<ClassifyRequest, ClassifyError> {
    let invalid = |message: &str| ClassifyError::new(ErrorKind::Invalid, message);
    let mut arguments = match arguments {
        Value::Object(map) => map,
        _ => return Err(invalid("classify expects an arguments object")),
    };
    if let Some(key) = arguments.keys().find(|key| !matches!(key.as_str(), "state" | "state_json" | "questions" | "model")) {
        return Err(ClassifyError::new(ErrorKind::Invalid, format!("classify: unknown argument '{key}'")));
    }
    let state_json = match arguments.get("state_json") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(invalid("state_json must be a boolean")),
    };
    let state = match (arguments.remove("state"), state_json) {
        (Some(Value::String(text)), false) => ClassifyState::Text(text),
        (Some(value @ (Value::Object(_) | Value::Array(_))), true) => ClassifyState::Json(value),
        (Some(Value::String(text)), true) => match serde_json::from_str::<Value>(&text) {
            Ok(value @ (Value::Object(_) | Value::Array(_))) => ClassifyState::Json(value),
            _ => return Err(invalid("state_json expects a JSON object or array")),
        },
        _ => return Err(invalid("state must be a string, or a JSON object/array with state_json")),
    };
    let questions: Map<String, Value> = match arguments.remove("questions") {
        Some(Value::Object(map)) => map,
        _ => return Err(invalid("questions must be a map of TypeSafe questions")),
    };
    let model = match arguments.remove("model") {
        None | Some(Value::Null) => DEFAULT_MODEL.to_string(),
        Some(Value::String(model)) => model,
        Some(_) => return Err(invalid("model must be a string")),
    };
    Ok(ClassifyRequest { state, questions, model, timeout_secs: DEFAULT_TIMEOUT_SECS, allow_secret_like: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noul() -> Value {
        json!({ "a": { "type": "noul", "instructions": "A?" } })
    }

    #[test]
    fn text_and_json_state_build_requests() {
        let text = request_from_arguments(json!({ "state": "hi", "questions": noul() })).expect("text");
        assert_eq!((text.state, text.model.as_str()), (ClassifyState::Text("hi".into()), DEFAULT_MODEL));
        let object = request_from_arguments(json!({ "state": { "k": 1 }, "state_json": true, "questions": noul(), "model": "jev-latest" }))
            .expect("json");
        assert_eq!(object.state, ClassifyState::Json(json!({ "k": 1 })));
        assert_eq!(object.model, "jev-latest");
        let encoded = request_from_arguments(json!({ "state": "[1,2]", "state_json": true, "questions": noul() })).expect("encoded");
        assert_eq!(encoded.state, ClassifyState::Json(json!([1, 2])));
        assert!(!encoded.allow_secret_like, "MCP callers cannot skip the secret screen");
    }

    #[test]
    fn malformed_arguments_are_invalid() {
        for bad in [
            json!({ "questions": noul() }),
            json!({ "state": { "k": 1 }, "questions": noul() }),
            json!({ "state": "x", "questions": [] }),
            json!({ "state": "x", "questions": noul(), "allow_secret_like": true }),
            json!({ "state": "nope", "state_json": true, "questions": noul() }),
            json!("string"),
        ] {
            assert_eq!(request_from_arguments(bad.clone()).unwrap_err().exit_code(), 4, "{bad}");
        }
    }

    #[tokio::test]
    async fn refusals_come_back_as_error_payloads() {
        let payload = classify_tool(json!({ "state": "token sk-liveA1b2C3d4E5f6G7h8", "questions": noul() })).await.expect("payload");
        assert!(payload["error"].as_str().expect("error").contains("secret-like"));
    }
}
