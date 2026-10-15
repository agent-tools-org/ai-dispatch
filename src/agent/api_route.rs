// Claude adapter overlay for named, independently billed API accounts.
// Claude retrieves its own key through a validated absolute-path apiKeyHelper.
use super::{Agent, RunOpts, claude::ClaudeAgent, custom::CustomAgentConfig};
use crate::config::api_route::ApiRoute;
use crate::routing::api_budget;
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub(crate) struct ApiRouteAgent(pub ApiRoute);

impl Agent for ApiRouteAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }
    fn rate_limit_name(&self) -> Option<&str> {
        Some(&self.0.id)
    }
    fn streaming(&self) -> bool {
        true
    }
    fn accepts_interactive_input(&self) -> bool {
        false
    }
    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = ClaudeAgent.build_command(prompt, opts)?;
        let settings = serde_json::json!({"apiKeyHelper": self.0.helper_command()?});
        cmd.args(["--settings", &serde_json::to_string(&settings)?]);
        cmd.env_remove("ANTHROPIC_API_KEY");
        Ok(cmd)
    }
    fn finalize_launch_env(&self, cmd: &mut Command) {
        cmd.env_remove("ANTHROPIC_API_KEY");
    }
    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        super::claude_events::parse_event_line_for_route(task_id, line, Some(&self.0.id))
    }
    fn parse_completion(&self, output: &str) -> CompletionInfo {
        ClaudeAgent.parse_completion(output)
    }
}

pub(crate) fn validate_launch(id: &str, store: &crate::store::Store) -> Result<()> {
    let Some(route) = crate::config::api_route::find(id)? else {
        return Ok(());
    };
    let budget = api_budget::for_route(&route, Some(store))?;
    if budget.billing.state == crate::routing::quota::billing::BillingState::Exhausted {
        anyhow::bail!("route {id}: {}", budget.label());
    }
    if let Some(hold) = crate::routing::rate_limit::dispatch_blocking_hold(&AgentKind::Custom, Some(id)) {
        anyhow::bail!("route {id} is held: {hold}");
    }
    Ok(())
}

pub(crate) fn stream_failure(agent: &dyn Agent, line: &str) -> Result<Option<String>> {
    let Some(id) = agent.rate_limit_name() else {
        return Ok(None);
    };
    if crate::config::api_route::find(id)?.is_none() {
        return Ok(None);
    }
    let cleaned = crate::runtime::watcher::strip_terminal_escapes(line);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(cleaned.trim()) else {
        return Ok(None);
    };
    let init = value["type"] == "system" && value["subtype"] == "init";
    if init && value["apiKeySource"] != "apiKeyHelper" {
        let reason = format!("route {id} is not billed to its API key");
        api_budget::hold(id, &reason, false)?;
        return Ok(Some(reason));
    }
    if credit_refusal(&value) {
        let reason = format!("route {id}: API credit balance exhausted (billing error)");
        api_budget::hold(id, &reason, true)?;
        return Ok(Some(reason));
    }
    Ok(None)
}

pub(crate) fn record_stream_failure(
    agent: &dyn Agent,
    line: &str,
    store: &crate::store::Store,
    task_id: &TaskId,
    info: &mut CompletionInfo,
) -> Result<bool> {
    let Some(reason) = stream_failure(agent, line)? else {
        return Ok(false);
    };
    if let Some(event) = agent.parse_event(task_id, line)
        && event.event_kind == EventKind::Completion
    {
        info.cost_usd = event
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cost_usd"))
            .and_then(serde_json::Value::as_f64)
            .or(info.cost_usd);
        store.insert_agent_event(&event)?;
    }
    info.status = TaskStatus::Failed;
    store.insert_agent_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail: reason,
        metadata: None,
    })?;
    Ok(true)
}

fn credit_refusal(value: &serde_json::Value) -> bool {
    let error = value["type"] == "error"
        || value["type"] == "result" && value["is_error"] == true
        || value["type"] == "assistant" && value.get("error").is_some();
    if !error {
        return false;
    }
    let text = value.to_string().to_ascii_lowercase().replace(['-', '_'], " ");
    [
        "credit balance is too low",
        "credit balance too low",
        "credit_balance_too_low",
        "insufficient credit",
        "billing_error",
        "billing error",
    ]
    .iter()
    .any(|s| text.contains(s))
}

pub(crate) fn registry_config(route: &ApiRoute) -> CustomAgentConfig {
    CustomAgentConfig {
        id: route.id.clone(),
        display_name: format!("{} (Claude API)", route.id),
        command: "claude".into(),
        prompt_mode: "arg".into(),
        prompt_flag: String::new(),
        dir_flag: String::new(),
        model_flag: "--model".into(),
        output_flag: String::new(),
        fixed_args: vec![],
        streaming: true,
        interactive_input: false,
        output_format: "jsonl".into(),
        capabilities: Default::default(),
        trust_tier: "api".into(),
        base_url: Some("https://api.anthropic.com".into()),
        provider: Some(format!("anthropic-api-{}", route.id)),
        metering: Some("spend_budget".into()),
        strengths: vec![],
        delegate_to: Some("claude".into()),
        forced_model: None,
        binary: None,
        extra_args: vec![],
        rate_limit_kind: None,
    }
}

#[cfg(test)]
#[path = "api_route_tests.rs"]
mod tests;
