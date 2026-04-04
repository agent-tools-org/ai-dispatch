// Stdio MCP server for `aid`, speaking JSON-RPC over line or framed transport.
// Exports run() and keeps transport parsing separate from tool dispatch.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::Arc;

use crate::cmd::mcp_tools;
use crate::store::Store;

const JSONRPC_VERSION: &str = "2.0";
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

enum TransportMode {
    JsonLine,
    Framed,
}

struct IncomingMessage {
    mode: TransportMode,
    body: String,
}

pub async fn run(store: Arc<Store>) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    while let Some(message) = read_message(&mut reader)? {
        if let Some(response) = handle_message(store.clone(), &message.body).await {
            write_response(&mut writer, &message.mode, &response)?;
            writer.flush()?;
        }
    }

    Ok(())
}

async fn handle_message(store: Arc<Store>, body: &str) -> Option<JsonRpcResponse> {
    let request = match serde_json::from_str::<JsonRpcRequest>(body) {
        Ok(request) => request,
        Err(err) => return Some(error_response(None, PARSE_ERROR, err.to_string())),
    };
    handle_request(store, request).await
}

async fn handle_request(store: Arc<Store>, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
    if request.jsonrpc != JSONRPC_VERSION {
        return Some(error_response(
            request.id,
            INVALID_REQUEST,
            format!("Unsupported jsonrpc '{}'", request.jsonrpc),
        ));
    }

    let id = request.id.clone();
    let result = match request.method.as_str() {
        "initialize" => Ok(initialize_result()),
        "tools/list" => Ok(json!({ "tools": mcp_tools::tool_definitions() })),
        "tools/call" => match serde_json::from_value::<ToolCallParams>(request.params) {
            Ok(params) => mcp_tools::call_tool(store, &params.name, params.arguments).await,
            Err(err) => Err(anyhow::anyhow!("Invalid tools/call params: {err}")),
        },
        "ping" => Ok(json!({})),
        method if method.starts_with("notifications/") => return None,
        other => {
            return id.map(|id| {
                error_response(
                    Some(id),
                    METHOD_NOT_FOUND,
                    format!("Unknown method '{other}'"),
                )
            });
        }
    };

    let response = match result {
        Ok(result) => success_response(id, result),
        Err(err) => error_response(id, INVALID_REQUEST, err.to_string()),
    };
    Some(response)
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "aid", "version": env!("CARGO_PKG_VERSION") },
        "tools": mcp_tools::tool_definitions()
    })
}

fn success_response(id: Option<Value>, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION,
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Option<Value>, code: i32, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION,
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    }
}

fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<IncomingMessage>> {
    let Some(first_line) = read_non_empty_line(reader)? else {
        return Ok(None);
    };
    if is_content_length_header(&first_line) {
        let body = read_framed_body(reader, &first_line)?;
        return Ok(Some(IncomingMessage {
            mode: TransportMode::Framed,
            body,
        }));
    }
    Ok(Some(IncomingMessage {
        mode: TransportMode::JsonLine,
        body: first_line,
    }))
}

fn read_non_empty_line<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
}

fn read_framed_body<R: BufRead>(reader: &mut R, first_line: &str) -> Result<String> {
    let mut content_length = parse_content_length(first_line)?;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if is_content_length_header(trimmed) {
            content_length = parse_content_length(trimmed)?;
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body).context("Invalid UTF-8 in framed MCP message")
}

fn write_response<W: Write>(
    writer: &mut W,
    mode: &TransportMode,
    response: &JsonRpcResponse,
) -> Result<()> {
    let body = serde_json::to_string(response)?;
    match mode {
        TransportMode::JsonLine => writer.write_all(format!("{body}\n").as_bytes())?,
        TransportMode::Framed => {
            writer.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes())?
        }
    }
    Ok(())
}

fn is_content_length_header(line: &str) -> bool {
    line.to_ascii_lowercase().starts_with("content-length:")
}

fn parse_content_length(line: &str) -> Result<usize> {
    line.split_once(':')
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid Content-Length header"))
}

#[cfg(test)]
mod tests {
    use super::{TransportMode, handle_message, read_message};
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;
    use std::io::Cursor;
    use std::sync::Arc;

    #[test]
    fn reads_json_line_messages() {
        let mut reader = Cursor::new(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        let message = read_message(&mut reader).unwrap().unwrap();
        assert!(matches!(message.mode, TransportMode::JsonLine));
        assert!(message.body.contains(r#""method":"tools/list""#));
    }

    #[test]
    fn reads_content_length_messages() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        let mut reader = Cursor::new(framed.into_bytes());
        let message = read_message(&mut reader).unwrap().unwrap();
        assert!(matches!(message.mode, TransportMode::Framed));
        assert_eq!(message.body, body);
    }

    #[tokio::test]
    async fn dispatches_board_tool_calls() {
        let store = Arc::new(Store::open_memory().unwrap());
        store.insert_task(&sample_task()).unwrap();

        let response = handle_message(
            store,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"aid_board","arguments":{}}}"#,
        )
        .await
        .unwrap();
        let result = response.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();

        assert!(text.contains("t-1234"));
        assert!(text.contains("codex"));
    }

    fn sample_task() -> Task {
        Task {
            id: TaskId("t-1234".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "Investigate the failing MCP test".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: Some(42),
            prompt_tokens: None,
            duration_ms: Some(1_500),
            model: Some("gpt-5".to_string()),
            cost_usd: Some(0.01),
            exit_code: None,
            created_at: Local::now(),
            completed_at: Some(Local::now()),
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        }
    }
}
