// App-server handshake, notification pumping, and persisted pending-message delivery.
// All RPC responses have deadlines; only turn/completed ends a successful run.

mod notifications;
mod sink;
use super::{
    Run,
    command::Launch,
    control,
    io::{RESPONSE_TIMEOUT, Rpc},
};
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
    last_progress: Instant,
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
            deadline: Instant::now() + run.policy.max_duration.min(run.policy.hard_cap),
            last_progress: Instant::now(),
            has_output: false,
        })
    }

    pub fn start(&mut self, launch: Launch) -> Result<()> {
        self.request(
            "initialize",
            json!({"clientInfo":{"name":"aid","version":env!("CARGO_PKG_VERSION")}}),
        )?;
        self.rpc.send(json!({"method":"initialized"}))?;
        let response = self.request(
            if launch.resume {
                "thread/resume"
            } else {
                "thread/start"
            },
            launch.thread,
        )?;
        self.thread = response["thread"]["id"]
            .as_str()
            .context("Missing Codex thread id")?
            .to_string();
        self.events.model = response["model"].as_str().map(str::to_string);
        self.sink.event(
            json!({"type":"thread.started","thread_id":self.thread,"model":self.events.model}),
        )?;
        let mut params =
            json!({"threadId":self.thread,"input":[{"type":"text","text":launch.prompt}]});
        if let Some(effort) = launch.effort {
            params["effort"] = effort;
        }
        let response = self.request("turn/start", params)?;
        self.turn = Some(
            response["turn"]["id"]
                .as_str()
                .context("Missing Codex turn id")?
                .to_string(),
        );
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.rpc.request(method, params)?;
        let deadline = (Instant::now() + RESPONSE_TIMEOUT).min(self.deadline);
        loop {
            if method != "turn/interrupt" && self.stopped()? {
                self.interrupt()?;
                bail!("Codex {method} cancelled because the task is stopping");
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "Codex {method} response timed out"
            );
            let Some(message) = self.rpc.receive(Duration::from_millis(100))? else {
                continue;
            };
            if message["id"].as_u64() == Some(id) && message.get("method").is_none() {
                if let Some(error) = message.get("error") {
                    bail!("Codex {method}: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .context("Missing Codex RPC result");
            }
            self.notification(message)?;
        }
    }

    pub fn pid(&self) -> u32 {
        self.rpc.pid()
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
        self.sink.finish()
    }

    fn monitor_until_completed(&mut self) -> Result<()> {
        let run = self.sink.run;
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
            let idle = if self.has_output {
                run.policy.idle
            } else {
                run.policy.first_token
            };
            anyhow::ensure!(
                self.last_progress.elapsed() < idle,
                "Codex app-server idle timeout"
            );
            if let Some(message) = self.rpc.receive(Duration::from_millis(100))? {
                self.notification(message)?;
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
        let expected = self
            .turn
            .clone()
            .context("Codex has no active steerable turn")?;
        let params = control::steer(&self.thread, Some(&expected), text)?;
        let response = self.request("turn/steer", params)?;
        control::accepted(&response, &expected)
    }

    fn interrupt(&mut self) -> Result<()> {
        if let Some(turn) = self.turn.clone() {
            self.request(
                "turn/interrupt",
                json!({"threadId":self.thread,"turnId":turn}),
            )?;
            let deadline = Instant::now() + Duration::from_secs(1);
            while !self.completed && Instant::now() < deadline {
                if let Some(message) = self.rpc.receive(Duration::from_millis(100))? {
                    self.notification(message)?;
                }
            }
        }
        self.sink.info.status = TaskStatus::Stopped;
        Ok(())
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
