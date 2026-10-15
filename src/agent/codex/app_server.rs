// Normalize Codex app-server notifications into the established exec event envelopes.
// Keeps cumulative usage and model evidence for the terminal turn event.

use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Events {
    pub usage: Value,
    pub model: Option<String>,
    baseline: Option<Value>,
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
            "item/started" | "item/completed" => item(message).into_iter().collect(),
            "turn/completed" => self.completed(params),
            "error" => vec![json!({"type":"error", "message":params["error"]["message"]})],
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

    fn completed(&self, params: &Value) -> Vec<Value> {
        let turn = &params["turn"];
        let mut events = vec![
            json!({"type":"turn.completed", "usage":if self.usage.is_null() {
            json!({"input_tokens":0,"output_tokens":0,"cached_input_tokens":0})
        } else { self.usage.clone() }, "model":self.model}),
        ];
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
        _ => return None,
    };
    item["type"] = json!(kind);
    Some(json!({"type":if completed {"item.completed"} else {"item.started"}, "item":item}))
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
            "rate_limit_reached"
                | "workspace_owner_credits_depleted"
                | "workspace_member_credits_depleted"
                | "workspace_owner_usage_limit_reached"
                | "workspace_member_usage_limit_reached"
        )
    );
    if exhausted.is_empty() && snapshot["spendControlReached"] != true && !reached {
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
