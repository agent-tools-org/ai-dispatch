// Verified worker/agent targets for stop; reused PIDs count as gone.
// Rechecks identity before every signal and waits without reaping any recorded PID.
use anyhow::{Result, bail};
use crate::runtime::background::{self, BackgroundRunSpec};
use std::time::{Duration, Instant};
use crate::runtime::execution_processes::probe;

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

enum Identity {
    Worker(String),
    Agent(Option<String>),
}

struct Target {
    pid: u32,
    identity: Identity,
}

pub(super) struct Targets(Vec<Target>);

impl Targets {
    pub(super) fn load(id: &str, spec: Option<&BackgroundRunSpec>) -> Result<Self> {
        let mut targets = Vec::new();
        if let Some(spec) = spec {
            anyhow::ensure!(
                spec.task_id == id,
                "Worker spec identity unverifiable for task {id}"
            );
            if let Some(pid) = spec.worker_pid.filter(|pid| Some(*pid) != spec.agent_pid) {
                targets.push(Target {
                    pid,
                    identity: Identity::Worker(id.into()),
                });
            }
            if let Some(pid) = spec.agent_pid {
                targets.push(Target {
                    pid,
                    identity: Identity::Agent(
                        spec.agent_started
                            .clone()
                            .filter(|value| !value.trim().is_empty()),
                    ),
                });
            }
        }
        let mut verified = Vec::new();
        for target in targets {
            if target.is_live()? {
                verified.push(target);
            }
        }
        Ok(Self(verified))
    }

    pub(super) fn has_live(&self) -> bool {
        !self.0.is_empty()
    }

    pub(super) fn stop(&self, graceful: bool) -> Result<bool> {
        let mut signalled = false;
        for target in &self.0 {
            signalled |= target.signal(!graceful)?;
        }
        for target in &self.0 {
            if target.wait_for_exit()? {
                continue;
            }
            if graceful {
                target.signal(true)?;
            }
            if !target.wait_for_exit()? {
                bail!("Process {} still alive after stop", target.pid);
            }
        }
        Ok(signalled)
    }
}

impl Target {
    fn is_live(&self) -> Result<bool> {
        if !probe::alive(self.pid)? {
            return Ok(false);
        }
        let matches = match &self.identity {
            Identity::Worker(id) => probe::field(self.pid, "args").map(|args| {
                args.split_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|pair| pair[0] == "__run-task" && pair[1] == id.as_str())
            }),
            Identity::Agent(Some(started)) => {
                match crate::runtime::process::monitor::activity::root_started(self.pid) {
                    Some(current) => Ok(current == *started),
                    None => Err(anyhow::anyhow!("Agent identity unreadable for pid {}", self.pid)),
                }
            }
            Identity::Agent(None) => Err(anyhow::anyhow!(
                "Agent identity unverifiable for pid {}: no recorded launch identity",
                self.pid
            )),
        };
        match matches {
            Err(_) if !probe::alive(self.pid)? => Ok(false),
            result => result,
        }
    }

    fn signal(&self, force: bool) -> Result<bool> {
        if !self.is_live()? {
            return Ok(false);
        }
        if force {
            background::sigkill_process(self.pid);
        } else {
            background::kill_process(self.pid);
        }
        Ok(true)
    }

    fn wait_for_exit(&self) -> Result<bool> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while self.is_live()? {
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        Ok(true)
    }
}
