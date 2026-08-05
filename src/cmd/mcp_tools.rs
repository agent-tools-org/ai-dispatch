// MCP tool definitions and handlers for the `aid mcp` stdio server.
// Exports tool_definitions() and call_tool(), reusing existing command logic.

use anyhow::{Context, Result};
use chrono::Local;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::agent::classifier::TaskCategory;
use crate::background;
use crate::cmd::advise;
use crate::cmd::agent_json;
use crate::cmd::ask;
use crate::cmd::mcp_schema;
use crate::cmd::retry::{self, RetryArgs};
use crate::cmd::run::{self, RunArgs};
use crate::cmd::show::{self, ShowMode};
use crate::config;
use crate::store::Store;
use crate::types::{
    DeclaredTaskProfile, Task, TaskBudget, TaskDifficulty, TaskFilter, TaskRigor, TaskUrgency,
};
use crate::usage;
use crate::usage_report;

#[cfg(test)]
#[path = "mcp_tools_tests.rs"]
mod tests;

pub fn tool_definitions() -> Vec<Value> {
    mcp_schema::tool_definitions()
}

pub async fn call_tool(store: Arc<Store>, name: &str, arguments: Value) -> Result<Value> {
    let payload = match match name {
        "aid_run" => run_tool(store, arguments).await,
        "aid_board" => board_tool(store, arguments),
        "aid_show" => show_tool(store, arguments),
        "aid_retry" => retry_tool(store, arguments).await,
        "aid_usage" => usage_tool(store),
        "aid_get_findings" => get_findings_tool(store, arguments),
        "aid_ask" => ask_tool(store, arguments).await,
        "aid_agents" => agents_tool(store),
        "aid_advise" => advise_tool(store, arguments),
        _ => Ok(error_payload(format!("Unknown tool '{name}'"))),
    } {
        Ok(payload) => payload,
        Err(err) => error_payload(err.to_string()),
    };
    Ok(tool_result(payload))
}

#[derive(Deserialize)]
struct RunToolArgs {
    agent: String,
    prompt: String,
    dir: Option<String>,
    worktree: Option<String>,
    #[serde(default = "default_true")]
    background: bool,
    model: Option<String>,
    group: Option<String>,
    verify: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Deserialize)]
struct BoardToolArgs {
    filter: Option<String>,
    group: Option<String>,
}

#[derive(Deserialize)]
struct ShowToolArgs {
    task_id: String,
    mode: Option<String>,
}

#[derive(Deserialize)]
struct RetryToolArgs {
    task_id: String,
    feedback: String,
    agent: Option<String>,
}

#[derive(Deserialize)]
struct AskToolArgs {
    question: String,
    agent: Option<String>,
}

#[derive(Deserialize)]
struct GetFindingsToolArgs {
    group: String,
}

#[derive(Deserialize)]
struct AdviseToolArgs {
    prompt: String,
    difficulty: String,
    budget: String,
    urgency: String,
    rigor: String,
    kind: Option<String>,
    team: Option<String>,
    #[serde(default = "default_top")]
    top: usize,
}

async fn run_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: RunToolArgs = parse_args(arguments, "aid_run")?;
    let task_id = run::run(
        store.clone(),
        RunArgs {
            agent_name: args.agent,
            prompt: args.prompt,
            dir: args.dir,
            model: args.model,
            worktree: args.worktree,
            group: args.group,
            verify: args.verify,
            skills: args.skills,
            background: args.background,
            ..Default::default()
        },
    )
    .await;
    match task_id {
        Ok(task_id) => {
            let status = store
                .get_task(task_id.as_str())?
                .map(|task| task.status.as_str().to_string())
                .unwrap_or_else(|| "pending".to_string());
            Ok(json!({ "task_id": task_id, "status": status }))
        }
        Err(err) => Ok(error_payload(err.to_string())),
    }
}

fn board_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: BoardToolArgs = parse_args(arguments, "aid_board")?;
    background::check_zombie_tasks(store.as_ref())?;
    let filter = parse_filter(args.filter.as_deref())?;
    let tasks = store
        .list_tasks(filter)?
        .into_iter()
        .filter(|task| matches_group(task, args.group.as_deref()))
        .map(render_board_task)
        .collect::<Vec<_>>();
    Ok(json!({ "tasks": tasks }))
}

fn show_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: ShowToolArgs = parse_args(arguments, "aid_show")?;
    let mode = parse_show_mode(args.mode.as_deref())?;
    let task = store
        .get_task(&args.task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", args.task_id))?;
    let content = match show::render_mode_text(&store, &args.task_id, mode) {
        Ok(content) => content,
        Err(err) => return Ok(error_payload(err.to_string())),
    };
    Ok(json!({ "task": task, "mode": mode_name(mode), "content": content }))
}

async fn retry_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: RetryToolArgs = parse_args(arguments, "aid_retry")?;
    let retry_id = retry::retry_task(
        store,
        RetryArgs {
            task_id: args.task_id,
            feedback: args.feedback,
            agent: args.agent,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .await;
    match retry_id {
        Ok(task_id) => Ok(json!({ "task_id": task_id })),
        Err(err) => Ok(error_payload(err.to_string())),
    }
}

fn usage_tool(store: Arc<Store>) -> Result<Value> {
    let config = config::load_config()?;
    let snapshot = usage::collect_usage(store.as_ref(), &config)?;
    let rendered = usage_report::render_usage(&snapshot);
    Ok(json!({ "snapshot": snapshot, "rendered": rendered }))
}

fn get_findings_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: GetFindingsToolArgs = parse_args(arguments, "aid_get_findings")?;
    let findings = store
        .get_workgroup_milestones(&args.group)?
        .into_iter()
        .map(|(task_id, finding)| json!({ "task_id": task_id, "finding": finding }))
        .collect::<Vec<_>>();
    Ok(json!(findings))
}

async fn ask_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: AskToolArgs = parse_args(arguments, "aid_ask")?;
    let answer = ask::ask_text(store, args.question, args.agent, None).await;
    match answer {
        Ok(answer) => Ok(json!({ "answer": answer })),
        Err(err) => Ok(error_payload(err.to_string())),
    }
}

fn agents_tool(store: Arc<Store>) -> Result<Value> {
    agent_json::agents_list_value(store.as_ref())
}

fn advise_tool(store: Arc<Store>, arguments: Value) -> Result<Value> {
    let args: AdviseToolArgs = parse_args(arguments, "aid_advise")?;
    let declared = DeclaredTaskProfile {
        difficulty: parse_dimension(&args.difficulty, "difficulty", TaskDifficulty::parse_str)?,
        budget: parse_dimension(&args.budget, "budget", TaskBudget::parse_str)?,
        urgency: parse_dimension(&args.urgency, "urgency", TaskUrgency::parse_str)?,
        rigor: parse_dimension(&args.rigor, "rigor", TaskRigor::parse_str)?,
    };
    let kind = match args.kind.as_deref() {
        Some(value) => Some(parse_dimension(value, "kind", TaskCategory::parse_str)?),
        None => None,
    };
    let report = advise::build_report(
        Some(store.as_ref()),
        &args.prompt,
        declared,
        kind,
        args.team.as_deref(),
        args.top,
    );
    Ok(serde_json::to_value(&report)?)
}

fn parse_dimension<T>(
    value: &str,
    name: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<T> {
    parse(value).ok_or_else(|| anyhow::anyhow!("Unknown {name} '{value}'"))
}

fn parse_args<T: for<'de> Deserialize<'de>>(arguments: Value, tool_name: &str) -> Result<T> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments).with_context(|| format!("Invalid arguments for {tool_name}"))
}

fn parse_filter(filter: Option<&str>) -> Result<TaskFilter> {
    match filter.unwrap_or("all") {
        "all" => Ok(TaskFilter::All),
        "today" => Ok(TaskFilter::Today),
        "running" => Ok(TaskFilter::Running),
        other => Err(anyhow::anyhow!("Unknown filter '{other}'")),
    }
}

fn parse_show_mode(mode: Option<&str>) -> Result<ShowMode> {
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

fn render_board_task(task: Task) -> Value {
    json!({
        "id": task.id,
        "agent": task.agent_display_name(),
        "status": task.status.as_str(),
        "duration": task_duration(&task),
        "tokens": task.tokens,
        "cost": task.cost_usd,
        "model": task.model,
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

fn mode_name(mode: ShowMode) -> &'static str {
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

fn matches_group(task: &Task, group: Option<&str>) -> bool {
    group.is_none_or(|group_id| task.workgroup_id.as_deref() == Some(group_id))
}

fn tool_result(payload: Value) -> Value {
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

fn error_payload(message: String) -> Value {
    json!({ "error": message })
}

fn default_true() -> bool {
    true
}

fn default_top() -> usize {
    5
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let safe = value.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &value[..safe])
    }
}
