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
        settings::reject_managed_settings()?;
        let mut cmd = ClaudeAgent.build_command(prompt, opts)?;
        let settings = settings::route_settings(&self.0, opts)?;
        cmd.args([
            "--setting-sources",
            "",
            "--settings",
            &serde_json::to_string(&settings)?,
        ]);
        scrub_env(&mut cmd);
        Ok(cmd)
    }
    fn finalize_launch_env(&self, cmd: &mut Command) {
        scrub_env(cmd);
    }
    fn api_route(&self) -> Option<&ApiRoute> {
        Some(&self.0)
    }
    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        super::claude_events::parse_event_line_for_route(task_id, line, Some(&self.0.id))
    }
    fn parse_completion(&self, output: &str) -> CompletionInfo {
        ClaudeAgent.parse_completion(output)
    }
}

pub(crate) fn validate_launch(
    agent: &dyn Agent,
    store: &crate::store::Store,
    container: bool,
) -> Result<()> {
    let Some(route) = agent.api_route() else {
        return Ok(());
    };
    reject_container(agent, container)?;
    let budget = api_budget::for_route(route, Some(store))?;
    if budget.billing.state == crate::routing::quota::billing::BillingState::Exhausted {
        anyhow::bail!("route {}: {}", route.id, budget.label());
    }
    if let Some(reason) = sweep_unverified_tasks(store, &route.id)? {
        anyhow::bail!(
            "route {} is held: {reason}; clear with `aid config clear-limit {}`",
            route.id,
            route.id
        );
    }
    if let Some(hold) =
        crate::routing::rate_limit::dispatch_blocking_hold(&AgentKind::Custom, Some(&route.id))
    {
        anyhow::bail!("route {} is held: {hold}", route.id);
    }
    Ok(())
}

pub(crate) fn reject_container(agent: &dyn Agent, container: bool) -> Result<()> {
    if let Some(route) = agent.api_route()
        && container
    {
        anyhow::bail!(
            "route {} does not support --container: its apiKeyHelper requires the host keychain",
            route.id
        );
    }
    Ok(())
}

pub(crate) const ROUTE_ENV_REMOVALS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

fn scrub_env(cmd: &mut Command) {
    for name in ROUTE_ENV_REMOVALS {
        cmd.env_remove(name);
    }
}

pub(crate) fn stream_failure(agent: &dyn Agent, line: &str) -> Result<Option<String>> {
    let Some(route) = agent.api_route() else {
        return Ok(None);
    };
    let id = &route.id;
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
    let reason = stream_failure(agent, line)?;
    let Some(reason) = reason else { return Ok(false) };
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
    store.insert_agent_event(&TaskEvent::new(task_id.clone(), EventKind::Error, reason))?;
    Ok(true)
}

fn sweep_unverified_tasks(store: &crate::store::Store, route: &str) -> Result<Option<String>> {
    let mut swept = None;
    for id in store.route_launch_task_ids(route)? {
        let Some(task) = store.get_task(&id)? else { continue };
        if !task.status.is_terminal() {
            continue;
        }
        let events = store.get_events(task.id.as_str())?;
        let Some(launch) = events.iter().rposition(|event| {
            event
                .metadata
                .as_ref()
                .is_some_and(|value| value.get("api_route_launch").is_some())
        }) else {
            continue;
        };
        if events[launch]
            .metadata
            .as_ref()
            .and_then(|value| value["api_route_launch"].as_str())
            != Some(route)
            || events[launch + 1..].iter().any(|event| {
                event.metadata.as_ref().is_some_and(|value| {
                    value["api_route_init_verified"] == true
                        || value["api_route_unverified_held"] == true
                })
            })
        {
            continue;
        }
        let reason = format!("route {route} is not billed to its API key");
        api_budget::hold(route, &reason, false)?;
        store.insert_event(&TaskEvent::new(task.id.clone(), EventKind::Error, reason.clone())
            .with_metadata(Some(serde_json::json!({"api_route_unverified_held": true}))))?;
        swept = Some(format!("{reason} (task {})", task.id.as_str()));
    }
    Ok(swept)
}

#[path = "api_route_refusal.rs"]
mod refusal;
#[path = "api_route_settings.rs"]
mod settings;
use refusal::credit_refusal;

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
