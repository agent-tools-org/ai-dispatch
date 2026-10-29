// Recorded task process evidence and non-reaping confirmation.
// Job specs retain identities for agents, descendants, and containers through cleanup.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use super::background::BackgroundRunSpec;
#[path = "execution_probe.rs"]
pub(crate) mod probe;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RecordedProcess {
    pub pid: u32,
    pub started: Option<String>,
}

pub(crate) fn confirmed_gone(process: &RecordedProcess) -> Result<bool> {
    if !probe::alive(process.pid)? {
        return Ok(true);
    }
    let started = process
        .started
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No recorded identity for pid {}", process.pid))?;
    let current = super::process::monitor::activity::root_started(process.pid)
        .ok_or_else(|| anyhow::anyhow!("Unreadable identity for pid {}", process.pid))?;
    Ok(current != *started)
}

pub(crate) fn worker_gone(pid: u32, id: &str) -> Result<bool> {
    if !probe::alive(pid)? {
        return Ok(true);
    }
    let args = probe::field(pid, "args")?;
    Ok(!args
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0] == "__run-task" && pair[1] == id))
}

pub(crate) fn snapshot(spec: &mut BackgroundRunSpec) {
    for root in [
        spec.worker_pid
            .filter(|pid| BackgroundRunSpec::is_task_worker(*pid, &spec.task_id)),
        spec.agent_pid_for_signal(),
    ]
    .into_iter()
    .flatten()
    {
        #[cfg(unix)]
        for pid in super::process::group::descendant_pids(root) {
            if Some(pid) != spec.agent_pid && !spec.tool_sessions.iter().any(|p| p.pid == pid) {
                spec.tool_sessions.push(RecordedProcess {
                    pid,
                    started: super::process::monitor::activity::root_started(pid),
                });
            }
        }
    }
}

pub(crate) fn capture(spec: &mut BackgroundRunSpec) -> Result<()> {
    snapshot(spec);
    super::background::save_spec(spec)
}

pub(crate) fn processes_gone(spec: &BackgroundRunSpec, worker: bool) -> Result<()> {
    if let Some(pid) = spec
        .worker_pid
        .filter(|pid| !worker || *pid != std::process::id())
    {
        anyhow::ensure!(worker_gone(pid, &spec.task_id)?, "Worker {pid} is alive");
    }
    if let Some(pid) = spec.agent_pid {
        anyhow::ensure!(
            confirmed_gone(&RecordedProcess {
                pid,
                started: spec.agent_started.clone(),
            })?,
            "Agent {pid} is alive"
        );
        // A dead leader can leave live members of its original process group.
        // A mismatched live leader instead identifies a reused group, which is not ours.
        if !probe::alive(pid)? {
            group_gone(pid)?;
        }
    }
    for process in &spec.tool_sessions {
        anyhow::ensure!(confirmed_gone(process)?, "Tool {} is alive", process.pid);
        if !probe::alive(process.pid)? {
            group_gone(process.pid)?;
        }
    }
    if let Some(name) = &spec.container_name {
        container_gone(name)?;
    }
    Ok(())
}

fn group_gone(pid: u32) -> Result<()> {
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "pgid=,stat="])
        .output()?;
    anyhow::ensure!(output.status.success(), "Agent group identity unreadable");
    for line in String::from_utf8(output.stdout)?.lines() {
        let mut fields = line.split_whitespace();
        if fields.next().and_then(|s| s.parse::<u32>().ok()) == Some(pid)
            && fields
                .next()
                .is_some_and(|s| !s.starts_with('Z') && !s.starts_with('X'))
        {
            bail!("Agent group {pid} is alive");
        }
    }
    Ok(())
}

fn container_gone(name: &str) -> Result<()> {
    #[cfg(test)]
    if let Some(value) = CONTAINER_LIST.with(|slot| slot.borrow().clone()) {
        return confirm_containers(name, &value);
    }
    let output = std::process::Command::new("container")
        .args(["list", "--all", "--format", "json"])
        .output()?;
    anyhow::ensure!(output.status.success(), "Container state unreadable for {name}");
    let containers: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    confirm_containers(name, &containers)
}

fn confirm_containers(name: &str, containers: &serde_json::Value) -> Result<()> {
    let entries = containers
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid container list"))?;
    for entry in entries {
        let id = entry
            .get("id")
            .or_else(|| entry.get("name"))
            .or_else(|| entry.pointer("/configuration/id"))
            .and_then(|v| v.as_str());
        anyhow::ensure!(id.is_some(), "Container identity unreadable");
        if id == Some(name) {
            let status = entry.get("status").and_then(|v| v.as_str());
            anyhow::ensure!(
                matches!(status, Some("stopped" | "exited")),
                "Container {name} is alive or unverifiable"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static CONTAINER_LIST: std::cell::RefCell<Option<serde_json::Value>> =
        const { std::cell::RefCell::new(None) };
}
