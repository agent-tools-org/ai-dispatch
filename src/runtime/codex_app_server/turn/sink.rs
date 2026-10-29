// Feed normalized app-server events through the existing streaming accounting pipeline.
// Persists exec-compatible logs and applies the same final delivery and quota guards.

use super::super::Run;
use crate::runtime::watcher::{self, StreamLineContext, SyntheticMilestoneTracker};
use crate::task::delivery_guard::{DeliveryEvidence, DeliveryOutcome};
use crate::types::*;
use anyhow::Result;
use serde_json::Value;
use std::io::Write;

pub(super) struct Sink<'a> {
    pub run: &'a Run<'a>,
    pub info: CompletionInfo,
    log: std::fs::File,
    tracker: SyntheticMilestoneTracker,
    delivery: DeliveryEvidence,
    pub(super) count: u32,
    pub(super) last_detail: Option<String>,
    saved: bool,
    quota: bool,
}

impl<'a> Sink<'a> {
    pub fn new(run: &'a Run<'a>) -> Result<Self> {
        Ok(Self {
            run,
            info: CompletionInfo::new(TaskStatus::Done),
            log: std::fs::File::create(run.log)?,
            tracker: SyntheticMilestoneTracker::new(),
            delivery: DeliveryEvidence::default(),
            count: 0,
            last_detail: None,
            saved: false,
            quota: false,
        })
    }

    pub fn event(&mut self, event: Value) -> Result<()> {
        let line = event.to_string();
        writeln!(self.log, "{line}")?;
        self.delivery.observe_codex_jsonl(&line);
        if event["type"] == "turn.usage" {
            self.usage(event)?;
            return self.check_cost();
        }
        if event["type"] == "codex.rate_limits" {
            return self.quota(&event["rate_limits"]);
        }
        let detail = watcher::handle_streaming_line_with_session(
            StreamLineContext {
                agent: self.run.agent,
                task_id: self.run.task,
                store: self.run.store,
                workgroup_id: self.run.group,
                synthetic_tracker: &mut self.tracker,
            },
            &mut self.info,
            &mut self.count,
            &line,
            &mut self.saved,
        )?;
        if let Some(detail) = detail {
            self.last_detail = Some(detail.detail);
        }
        if let Some(message) = crate::agent::codex::app_server::error_quota_message(&event) {
            crate::routing::rate_limit::mark_rate_limited_for_model_value(
                &AgentKind::Codex,
                self.run.agent.rate_limit_name(),
                self.info.model.as_deref(),
                &event,
                &message,
            );
            self.quota = true;
        }
        if event["type"] == "turn.failed" && self.info.status != TaskStatus::Stopped {
            self.info.status = TaskStatus::Failed;
        }
        self.check_cost()
    }

    fn usage(&mut self, mut value: Value) -> Result<()> {
        value["type"] = serde_json::json!("turn.completed");
        if let Some(mut event) = self
            .run
            .agent
            .parse_event(self.run.task, &value.to_string())
        {
            self.info.tokens = event
                .metadata
                .as_ref()
                .and_then(|metadata| metadata["tokens"].as_i64());
            crate::cost::apply_completion_usage_cost(
                self.run.store,
                self.run.task,
                &mut self.info,
                &event,
                AgentKind::Codex,
            )?;
            event.event_kind = EventKind::Milestone;
            self.run.store.insert_agent_event_with_usage(&event, &self.info)?;
        }
        Ok(())
    }

    pub(super) fn check_cost(&self) -> Result<()> {
        anyhow::ensure!(
            !matches!((self.info.cost_usd,self.run.max_cost),
            (Some(cost),Some(limit)) if cost > limit),
            "Codex task exceeded cost ceiling"
        );
        Ok(())
    }

    fn quota(&mut self, snapshot: &Value) -> Result<()> {
        if self.run.agent.rate_limit_name().is_some() {
            return Ok(());
        }
        self.run.store.insert_agent_event(&TaskEvent::new(
            self.run.task.clone(),
            EventKind::Milestone,
            "Codex rate-limit evidence updated".to_string(),
        ).with_metadata(Some(serde_json::json!({"codex_rate_limits":snapshot}))))?;
        if let Some(message) = crate::agent::codex::app_server::quota_message(snapshot) {
            crate::routing::rate_limit::mark_rate_limited_for_model_value(
                &AgentKind::Codex,
                self.run.agent.rate_limit_name(),
                self.info.model.as_deref(),
                snapshot,
                &message,
            );
            self.quota = true;
        }
        Ok(())
    }

    fn failure_stderr(&mut self) -> Result<()> {
        if self.info.status != TaskStatus::Failed {
            return Ok(());
        }
        let Some(stderr) = watcher::read_capped_stderr(self.run.task.as_str()) else {
            return Ok(());
        };
        if let Some(message) = crate::routing::rate_limit::refusal_on_channel(
            &stderr,
            AgentKind::Codex,
            crate::routing::quota::channel::Channel::CliStderr,
        ) {
            crate::routing::rate_limit::mark_rate_limited_for_model(
                &AgentKind::Codex,
                self.run.agent.rate_limit_name(),
                self.info.model.as_deref(),
                &message,
            );
            self.quota = true;
        }
        let (detail, metadata) = crate::agent::truncate::capped_detail(&stderr);
        self.run.store.insert_agent_event(&TaskEvent::new(
            self.run.task.clone(),
            EventKind::Error,
            detail,
        ).with_metadata(metadata))?;
        writeln!(
            self.log,
            "{}",
            serde_json::json!({"type":"error","source":"stderr","message":stderr})
        )?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<CompletionInfo> {
        let delivered = matches!(self.delivery.validate(), DeliveryOutcome::Delivered);
        self.info.status = watcher::apply_codex_delivery_guard(
            self.run.store,
            self.run.task,
            self.info.status,
            self.delivery.validate(),
            None,
        );
        if !delivered && self.quota {
            self.info.status = TaskStatus::Failed;
        }
        self.failure_stderr()?;
        self.log.flush()?;
        if self.info.status == TaskStatus::Done && !self.quota {
            crate::routing::rate_limit::clear_rate_limit_for_model(
                &AgentKind::Codex,
                self.run.agent.rate_limit_name(),
                self.info.model.as_deref(),
            );
        }
        self.info.exit_code = None;
        Ok(self.info)
    }
}

#[cfg(test)]
#[path = "sink_scope_tests.rs"]
mod scope_tests;

#[cfg(test)]
#[path = "sink_provider_tests.rs"]
mod provider_tests;
