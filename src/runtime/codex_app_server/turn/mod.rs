// App-server handshake, notification pumping, and persisted pending-message delivery.
// All RPC responses have deadlines; only turn/completed ends a successful run.

mod finish;
mod notifications;
mod requests;
mod sink;
mod startup;
mod watchdog;
use super::{Run, command::Launch, control, io::Rpc};
use crate::agent::codex::app_server::Events;
use crate::types::*;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sink::Sink;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub(super) struct Turn<'a> {
    rpc: Rpc,
    sink: Sink<'a>,
    events: Events,
    thread: String,
    turn: Option<String>,
    completed: bool,
    attempted: HashSet<i64>,
    deadline: Instant,
    last_progress: std::time::SystemTime,
    startup_deadline: Instant,
    start_requested: bool,
    terminal: Option<Value>,
    has_output: bool,
}

impl<'a> Turn<'a> {
    pub fn new(run: &'a Run<'a>, rpc: Rpc) -> Result<Self> {
        Ok(Self {
            rpc,
            sink: Sink::new(run)?,
            events: Events::default(),
            thread: String::new(),
            turn: None,
            completed: false,
            attempted: HashSet::new(),
            deadline: run.start + run.policy.max_duration.min(run.policy.hard_cap),
            last_progress: std::time::SystemTime::now(),
            startup_deadline: Instant::now()
                + Duration::from_secs(10)
                    .min(run.policy.idle)
                    .min(run.policy.max_duration.min(run.policy.hard_cap)),
            start_requested: false,
            terminal: None,
            has_output: false,
        })
    }

    pub fn monitor(mut self) -> Result<CompletionInfo> {
        if let Err(error) = self.monitor_until_completed() {
            self.sink.info.status = TaskStatus::Failed;
            let detail = format!("Codex app-server: {error:#}");
            self.sink
                .event(json!({"type":"error","message":detail}))
                .ok();
            self.refuse_pending(&detail)?;
        } else {
            self.refuse_pending("Codex turn is finished; no active turn")?;
        }
        if let Err(error) = self.flush_terminal() {
            self.sink.info.status = TaskStatus::Failed;
            self.sink
                .event(json!({"type":"error","message":error.to_string()}))
                .ok();
        }
        self.sink.finish()
    }

    fn monitor_until_completed(&mut self) -> Result<()> {
        while !self.completed {
            anyhow::ensure!(
                Instant::now() < self.deadline,
                "Codex task exceeded hard timeout"
            );
            if self.stopped()? {
                self.interrupt()?;
                break;
            }
            self.pending()?;
            self.sink.check_cost()?;
            if self.completed {
                break;
            }
            if let Some(message) = self.rpc.receive(Duration::from_millis(100))? {
                self.notification(message)?;
            }
            if !self.completed {
                self.check_idle()?;
            }
        }
        Ok(())
    }

    fn stopped(&self) -> Result<bool> {
        Ok(self
            .sink
            .run
            .control
            .is_some_and(|control| control.is_interrupted())
            || self
                .sink
                .run
                .store
                .get_task(self.sink.run.task.as_str())?
                .is_none_or(|task| task.status.is_terminal()))
    }

    fn pending(&mut self) -> Result<()> {
        let run = self.sink.run;
        for message in run.store.pending_inbound_for_task(run.task.as_str())? {
            if !self.attempted.insert(message.id) {
                continue;
            }
            let result = self.deliver(&message.content);
            control::record(run.store, run.task, message.id, result)?;
        }
        Ok(())
    }

    fn deliver(&mut self, text: &str) -> Result<String> {
        anyhow::ensure!(!self.completed, "Codex turn is finished; no active turn");
        let expected = self
            .turn
            .clone()
            .context("Codex has no active steerable turn")?;
        let params = control::steer(&self.thread, Some(&expected), text)?;
        let response = self.request("turn/steer", params)?;
        control::accepted(&response, &expected)
    }

    fn refuse_pending(&mut self, detail: &str) -> Result<()> {
        let run = self.sink.run;
        for message in run.store.pending_inbound_for_task(run.task.as_str())? {
            if self.attempted.insert(message.id) {
                control::record(
                    run.store,
                    run.task,
                    message.id,
                    Err(anyhow::anyhow!("{detail}")),
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
