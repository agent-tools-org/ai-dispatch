// MCP tool schema definitions for the `aid mcp` stdio server.
// Exports tool_definitions() so transport and handlers stay compact.

use serde_json::{json, Value};

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
        tool(
            "aid_agents",
            "List the agent fleet: state, quota, capabilities, models, history.",
            empty_schema(),
        ),
        tool(
            "aid_advise",
            "Read-only routing advice for a declared task profile; dispatches nothing.",
            advise_schema(),
        ),
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
            "verify": { "type": "string" },
            "skills": { "type": "array", "items": { "type": "string" }, "description": "Methodology skills to inject" }
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
            "mode": { "type": "string", "enum": ["summary", "stat", "events", "diff", "output", "log"] }
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
            "feedback": { "type": "string" },
            "agent": { "type": "string", "description": "Override agent for retry" }
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

fn advise_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "prompt": { "type": "string" },
            "difficulty": { "type": "string", "enum": ["trivial", "simple", "moderate", "complex"] },
            "budget": { "type": "string", "enum": ["free", "cheap", "standard", "premium"] },
            "urgency": { "type": "string", "enum": ["background", "normal", "urgent"] },
            "rigor": { "type": "string", "enum": ["draft", "standard", "critical"] },
            "kind": {
                "type": "string",
                "enum": [
                    "research", "simple-edit", "complex-impl", "frontend",
                    "debugging", "testing", "refactoring", "documentation"
                ]
            },
            "team": { "type": "string" },
            "top": { "type": "integer", "default": 5, "description": "Candidate limit; 0 = all" }
        },
        "required": ["prompt", "difficulty", "budget", "urgency", "rigor"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::tool_definitions;

    #[test]
    fn registers_advice_surfaces_beside_existing_tools() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        for expected in [
            "aid_run", "aid_board", "aid_show", "aid_retry", "aid_usage",
            "aid_get_findings", "aid_ask", "aid_agents", "aid_advise",
        ] {
            assert!(names.contains(&expected), "missing tool '{expected}'");
        }
    }

    #[test]
    fn advise_schema_requires_all_declared_dimensions() {
        let advise = tool_definitions()
            .into_iter()
            .find(|tool| tool["name"] == "aid_advise")
            .expect("aid_advise registered");
        let required: Vec<&str> = advise["inputSchema"]["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        for dimension in ["prompt", "difficulty", "budget", "urgency", "rigor"] {
            assert!(required.contains(&dimension), "missing required '{dimension}'");
        }
    }
}
