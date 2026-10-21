// Codex adapter carrying only a persisted route name, never provider settings.
// Verifies the live selection before constructing commands and scopes stream holds.
use super::{CodexAgent, provider};
use crate::agent::{Agent, CommandContext, RunOpts};
use crate::types::*;
use anyhow::Result;
use std::process::Command;

pub(crate) fn verify_route(route: Option<&str>) -> Result<()> {
    let live = provider::route_name()?;
    anyhow::ensure!(
        live.as_deref() == route,
        "config.toml now selects {}; task recorded {}; select that route before retry/resume",
        live.as_deref().unwrap_or("openai-chatgpt-plan"),
        route.unwrap_or("openai-chatgpt-plan"),
    );
    Ok(())
}

pub(crate) fn routed(route: Option<String>) -> Box<dyn Agent> {
    Box::new(Routed { route })
}

struct Routed {
    route: Option<String>,
}

impl Agent for Routed {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }
    fn rate_limit_name(&self) -> Option<&str> {
        self.route.as_deref()
    }
    fn streaming(&self) -> bool {
        CodexAgent.streaming()
    }
    fn accepts_interactive_input(&self) -> bool {
        CodexAgent.accepts_interactive_input()
    }
    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        verify_route(self.rate_limit_name())?;
        CodexAgent.build_command(prompt, opts)
    }
    fn build_command_with_context(
        &self,
        prompt: &str,
        opts: &RunOpts,
        context: CommandContext,
    ) -> Result<Command> {
        verify_route(self.rate_limit_name())?;
        CodexAgent.build_command_with_context(prompt, opts, context)
    }
    fn validate_cli(
        &self,
        model: Option<&str>,
        run: &crate::agent::CliCommandRunner<'_>,
    ) -> Result<()> {
        CodexAgent.validate_cli(model, run)
    }
    fn served_models_fingerprint(&self) -> Option<String> {
        CodexAgent.served_models_fingerprint()
    }
    fn served_models(&self) -> Result<Option<Vec<String>>> {
        CodexAgent.served_models()
    }
    fn parse_event(&self, task: &TaskId, line: &str) -> Option<TaskEvent> {
        super::event_route::parse_event(task, line, self.rate_limit_name())
    }
}
