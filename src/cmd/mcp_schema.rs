// MCP tool schema definitions for the `aid mcp` stdio server.
// Exports tool_definitions() so transport and handlers stay compact.

use serde_json::{Value, json};

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool("aid_run", "Dispatch a task to an AI agent.", run_schema()),
        tool("aid_board", "List tracked tasks.", board_schema()),
        tool(
            "aid_show",
            "Inspect one task and its artifacts.",
            show_schema(),
        ),
        tool(
            "aid_retry",
            "Retry a failed task with feedback.",
            retry_schema(),
        ),
        tool(
            "aid_usage",
            "Show tracked usage and budget status.",
            empty_schema(),
        ),
        tool(
            "aid_get_findings",
            "List milestone findings shared within a workgroup.",
            get_findings_schema(),
        ),
        tool("aid_ask", "Run a quick research query.", ask_schema()),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn run_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agent": { "type": "string" },
            "prompt": { "type": "string" },
            "dir": { "type": "string" },
            "worktree": { "type": "string" },
            "background": { "type": "boolean", "default": true },
            "model": { "type": "string" },
            "group": { "type": "string" },
            "verify": { "type": "string" }
        },
        "required": ["agent", "prompt"],
        "additionalProperties": false
    })
}

fn board_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "filter": { "type": "string", "enum": ["all", "today", "running"] },
            "group": { "type": "string" }
        },
        "additionalProperties": false
    })
}

fn show_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": { "type": "string" },
            "mode": { "type": "string", "enum": ["summary", "diff", "output", "log"] }
        },
        "required": ["task_id"],
        "additionalProperties": false
    })
}

fn retry_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": { "type": "string" },
            "feedback": { "type": "string" }
        },
        "required": ["task_id", "feedback"],
        "additionalProperties": false
    })
}

fn ask_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": { "type": "string" },
            "agent": { "type": "string", "default": "gemini" }
        },
        "required": ["question"],
        "additionalProperties": false
    })
}

fn get_findings_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "group": { "type": "string" }
        },
        "required": ["group"],
        "additionalProperties": false
    })
}
