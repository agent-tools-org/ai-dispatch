// Normalize Codex app-server notifications into the established exec event envelopes.
// Keeps cumulative usage and model evidence for the terminal turn event.

use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Events {
    pub usage: Value,
    pub model: Option<String>,
    baseline: Option<Value>,
    completed_items: std::collections::HashSet<String>,
}

impl Events {
    pub(crate) fn notification(&mut self, message: &Value) -> Vec<Value> {
        let params = &message["params"];
        match message["method"].as_str().unwrap_or_default() {
            "thread/started" => vec![json!({"type":"thread.started",
                "thread_id":params["thread"]["id"], "model":self.model})],
            "thread/tokenUsage/updated" => {
                self.usage = self.turn_usage(&params["tokenUsage"]);
                vec![json!({"type":"turn.usage", "usage":self.usage, "model":self.model})]
            }
            "item/started" | "item/completed" => {
                let event = item(message);
                if event.is_some() && message["method"] == "item/completed" {
                    if let Some(id) = params["item"]["id"].as_str() {
                        self.completed_items.insert(id.to_string());
                    }
                }
                event.into_iter().collect()
            }
            "turn/completed" => self.completed(params),
            "error" => vec![json!({"type":"error", "message":params["error"]["message"],
                "error":params["error"]})],
            "account/rateLimits/updated" => vec![json!({"type":"codex.rate_limits",
                "rate_limits":params["rateLimits"]})],
            _ => Vec::new(),
        }
    }

    fn turn_usage(&mut self, tokens: &Value) -> Value {
        let total = usage(&tokens["total"]);
        let last = usage(&tokens["last"]);
        let baseline = self.baseline.get_or_insert_with(|| subtract(&total, &last));
        subtract(&total, baseline)
    }

    fn final_items(&mut self, turn: &Value) -> Vec<Value> {
        let mut events = Vec::new();
        for value in turn["items"].as_array().into_iter().flatten() {
            if let Some(id) = value["id"].as_str() {
                if !self.completed_items.insert(id.to_string()) {
                    continue;
                }
            }
            if let Some(event) = item(&json!({"method":"item/completed","params":{"item":value}})) {
                events.push(event);
            }
        }
        events
    }

    fn completed(&mut self, params: &Value) -> Vec<Value> {
        let turn = &params["turn"];
        let mut events = self.final_items(turn);
        events.push(
            json!({"type":"turn.completed", "usage":if self.usage.is_null() {
            json!({"input_tokens":0,"output_tokens":0,"cached_input_tokens":0})
        } else { self.usage.clone() }, "model":self.model}),
        );
        if turn["status"] != "completed" || !turn["error"].is_null() {
            let message = turn["error"]["message"]
                .as_str()
                .unwrap_or("Codex turn did not complete");
            events.push(json!({"type":"turn.failed", "message":message, "error":turn["error"]}));
        }
        events
    }
}

fn subtract(total: &Value, baseline: &Value) -> Value {
    let mut value = json!({});
    for key in ["input_tokens", "cached_input_tokens", "output_tokens"] {
        value[key] = json!(
            total[key]
                .as_i64()
                .unwrap_or(0)
                .saturating_sub(baseline[key].as_i64().unwrap_or(0))
                .max(0)
        );
    }
    value
}

fn usage(total: &Value) -> Value {
    json!({"input_tokens":total["inputTokens"], "cached_input_tokens":total["cachedInputTokens"],
        "output_tokens":total["outputTokens"]})
}

fn item(message: &Value) -> Option<Value> {
    let completed = message["method"] == "item/completed";
    let mut item = message["params"]["item"].clone();
    let kind = match item["type"].as_str()? {
        "agentMessage" if completed => "agent_message",
        "reasoning" if completed => {
            item["text"] = json!(reasoning(&item));
            "reasoning"
        }
        "commandExecution" => {
            item["exit_code"] = item["exitCode"].clone();
            item["aggregated_output"] = item["aggregatedOutput"].clone();
            "command_execution"
        }
        "fileChange" if completed => "file_change",
        "mcpToolCall" => "mcp_tool_call",
        "webSearch" => "web_search",
        "collabAgentToolCall" => "collab_tool_call",
        "imageGeneration" => "image_generation",
        "plan" => "todo_list",
        _ => return generic_item(message),
    };
    item["type"] = json!(kind);
    let envelope = if kind == "reasoning" {
        "codex.reasoning"
    } else if completed {
        "item.completed"
    } else {
        "item.started"
    };
    Some(json!({"type":envelope, "item":item}))
}

fn generic_item(message: &Value) -> Option<Value> {
    let mut item = message["params"]["item"].clone();
    let kind = item["type"].as_str()?;
    if kind == "userMessage" || kind == "agentMessage" || kind == "reasoning" {
        return None;
    }
    let mut normalized = String::new();
    for ch in kind.chars() {
        if ch.is_uppercase() {
            normalized.push('_');
        }
        normalized.extend(ch.to_lowercase());
    }
    item["type"] = json!(normalized);
    Some(
        json!({"type":if message["method"] == "item/completed" {"item.completed"}
        else {"item.started"},"item":item}),
    )
}

fn reasoning(item: &Value) -> String {
    ["summary", "content"]
        .into_iter()
        .filter_map(|key| item[key].as_array())
        .flatten()
        .filter_map(|text| text.as_str().or_else(|| text["text"].as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn quota_message(snapshot: &Value) -> Option<String> {
    let exhausted: Vec<_> = ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| snapshot.get(key))
        .filter(|window| {
            window["usedPercent"]
                .as_f64()
                .is_some_and(|used| used >= 100.0)
        })
        .collect();
    let reached = matches!(
        snapshot["rateLimitReachedType"].as_str(),
        Some(
            "workspace_owner_credits_depleted"
                | "workspace_member_credits_depleted"
                | "workspace_owner_usage_limit_reached"
                | "workspace_member_usage_limit_reached"
        )
    );
    let overage =
        snapshot["credits"]["hasCredits"] == true || snapshot["credits"]["unlimited"] == true;
    let window = !exhausted.is_empty() || snapshot["rateLimitReachedType"] == "rate_limit_reached";
    if (!window || overage) && snapshot["spendControlReached"] != true && !reached {
        return None;
    }
    let reset = exhausted
        .iter()
        .filter_map(|window| window["resetsAt"].as_i64())
        .max()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%b %d, %Y %I:%M %p")
                .to_string()
        });
    Some(match reset {
        Some(reset) => format!("Codex rate limit exceeded. try again at {reset}"),
        None => "Codex rate limit exceeded".to_string(),
    })
}

#[cfg(test)]
#[path = "app_server_tests.rs"]
mod tests;

/// Typed app-server errors own their code; unrelated text fields cannot supply one.
pub(crate) fn error_quota_message(event: &Value) -> Option<String> {
    if !matches!(event["type"].as_str(), Some("error" | "turn.failed")) {
        return None;
    }
    let error = &event["error"];
    let code = &error["codexErrorInfo"];
    if code == "usageLimitExceeded" {
        return Some("You have hit your usage limit".to_string());
    }
    if code == "httpConnectionFailed"
        || code.get("httpConnectionFailed").is_some()
    {
        let status = code["httpConnectionFailed"]["httpStatusCode"].as_i64();
        return matches!(status, Some(429 | 402)).then(|| "Codex rate limit exceeded".to_string());
    }
    if !code.is_null() {
        return None;
    }
    let message = error["message"].as_str().or_else(|| event["message"].as_str())?;
    crate::routing::rate_limit::is_rate_limit_error_for_agent(message, &crate::types::AgentKind::Codex)
        .then(|| message.to_string())
}
