// MCP argument parsing and result rendering helpers.
// Deps: serde, task types, show modes, and UI formatting.

use super::*;

pub(super) fn parse_dimension<T>(
    value: &str,
    name: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<T> {
    parse(value).ok_or_else(|| anyhow::anyhow!("Unknown {name} '{value}'"))
}

pub(super) fn parse_args<T: for<'de> Deserialize<'de>>(
    arguments: Value,
    tool_name: &str,
) -> Result<T> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments).with_context(|| format!("Invalid arguments for {tool_name}"))
}

pub(super) fn parse_filter(filter: Option<&str>) -> Result<TaskFilter> {
    match filter.unwrap_or("all") {
        "all" => Ok(TaskFilter::All),
        "today" => Ok(TaskFilter::Today),
        "running" => Ok(TaskFilter::Running),
        other => Err(anyhow::anyhow!("Unknown filter '{other}'")),
    }
}

pub(super) fn parse_show_mode(mode: Option<&str>) -> Result<ShowMode> {
    match mode.unwrap_or("summary") {
        "summary" => Ok(ShowMode::Default),
        "stat" => Ok(ShowMode::Summary),
        "context" => Ok(ShowMode::Context),
        "events" => Ok(ShowMode::Events),
        "diff" => Ok(ShowMode::Diff),
        "output" => Ok(ShowMode::Output),
        "log" => Ok(ShowMode::Log),
        other => Err(anyhow::anyhow!("Unknown show mode '{other}'")),
    }
}

pub(super) fn render_board_task(task: Task) -> Value {
    json!({
        "id": task.id,
        "agent": task.agent_display_name(),
        "status": task.status.as_str(),
        "outcome": task.outcome().as_str(),
        "verify_status": task.verify_status.as_str(),
        "duration": task_duration(&task),
        "tokens": task.tokens,
        "cost": task.cost_usd,
        "requested_model": task.requested_model,
        "observed_model": task.observed_model,
        "attribution_source": task.attribution_source.map(|value| value.as_str()),
        "prompt_preview": truncate(&task.prompt, 80),
    })
}

fn task_duration(task: &Task) -> String {
    match task.duration_ms {
        Some(ms) => format_duration(ms),
        None => format_elapsed(Local::now() - task.created_at),
    }
}

fn format_duration(ms: i64) -> String {
    format_elapsed(chrono::Duration::milliseconds(ms))
}

fn format_elapsed(duration: chrono::Duration) -> String {
    let secs = duration.num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

pub(super) fn mode_name(mode: ShowMode) -> &'static str {
    match mode {
        ShowMode::Default => "summary",
        ShowMode::Summary => "stat",
        ShowMode::Events => "events",
        ShowMode::Context => "context",
        ShowMode::Diff => "diff",
        ShowMode::Output => "output",
        ShowMode::Transcript => "transcript",
        ShowMode::Log => "log",
    }
}

pub(super) fn tool_result(payload: Value) -> Value {
    let is_error = payload.get("error").is_some();
    let mut result = json!({ "content": [{ "type": "text", "text": render_payload(payload) }] });
    if is_error {
        result["isError"] = json!(true);
    }
    result
}

fn render_payload(payload: Value) -> String {
    serde_json::to_string_pretty(&payload).unwrap_or_else(|err| format!(r#"{{"error":"{err}"}}"#))
}

pub(super) fn error_payload(message: String) -> Value {
    json!({ "error": message })
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_top() -> usize {
    5
}
