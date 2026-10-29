// Execution receipts derive settlement from the existing job-spec barrier.
// Agent launch evidence survives spec removal in the task row; probes never reap processes.
use anyhow::Result;
use serde_json::{Value, json};
use crate::{
    runtime::{background, execution_probe as probe},
    store::Store,
};

pub(crate) fn receipt(store: &Store, id: &str) -> Result<Value> {
    derive_settlement(store, id)?;
    let mut receipt = store
        .execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    receipt["settled"] = json!(receipt["worker"]["state"] == "settled");
    receipt["observed_at"] = json!(chrono::Utc::now().to_rfc3339());
    Ok(receipt)
}

pub(crate) fn derive_settlement(store: &Store, id: &str) -> Result<bool> {
    let facts = store
        .execution_facts(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    if facts.settlement.is_some_and(|worker| worker.state == "settled") {
        return Ok(false);
    }
    let observed = observe(store, id);
    match observed {
        Ok(Some(outcome)) => store.persist_settlement(id, outcome),
        Ok(None) => Ok(false),
        Err(error) => {
            store.observe_worker_state(id, "unconfirmed", None, Some(error.to_string()))?;
            Ok(false)
        }
    }
}

fn observe(store: &Store, id: &str) -> Result<Option<&'static str>> {
    let spec = background::load_spec_if_exists(id)?;
    if let Some(spec) = &spec {
        anyhow::ensure!(spec.task_id == id, "Worker spec identity mismatch");
        if let Some(pid) = spec.worker_pid {
            store.mark_worker_spawned(id)?;
            if !worker_gone(pid, id)? {
                store.observe_worker_state(id, "running", Some(pid), None)?;
                return Ok(None);
            }
        } else {
            store.observe_worker_state(id, "pending", None, None)?;
            return Ok(None);
        }
    }
    let (agent, spawned, stopped) = store.execution_process(id)?;
    anyhow::ensure!(
        !(spec.as_ref().is_some_and(|spec| spec.agent_pid.is_some())
            && agent.as_ref().is_none_or(|agent| agent.state == "not_started")),
        "Agent launch evidence missing"
    );
    agent_group_gone(agent.as_ref())?;
    let task = store
        .get_task(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    if !spawned && agent.as_ref().is_some_and(|a| a.state == "not_started") && !task.status.is_terminal() {
        return Ok(None);
    }
    Ok(Some(
        if stopped || task.status == crate::types::TaskStatus::Stopped {
            "stopped"
        } else if !spawned && agent.as_ref().is_some_and(|a| a.state == "not_started") {
            "launch_failed"
        } else if spec.is_some() {
            "lost"
        } else {
            "completed"
        },
    ))
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

pub(crate) fn agent_group_gone(agent: Option<&crate::store::execution_protocol::AgentProcess>) -> Result<()> {
    let agent = agent.ok_or_else(|| anyhow::anyhow!("No recorded agent process group"))?;
    if agent.state == "not_started" {
        return Ok(());
    }
    let (Some(pid), Some(pgid), Some(started)) = (agent.pid, agent.pgid, agent.started.as_deref()) else {
        anyhow::bail!("Agent process group or launch identity missing");
    };
    anyhow::ensure!(
        pgid > 1 && pgid <= i32::MAX as u32 && !started.is_empty(),
        "Invalid agent identity"
    );
    if probe::alive(pid)? {
        let current = crate::runtime::process::monitor::activity::root_started(pid)
            .ok_or_else(|| anyhow::anyhow!("Agent identity unreadable"))?;
        if current != started {
            return Ok(());
        }
    }
    group_gone(pgid)
}

#[cfg(unix)]
pub(crate) fn group_gone(pgid: u32) -> Result<()> {
    let result = unsafe { libc::kill(-(pgid as i32), 0) };
    let error = std::io::Error::last_os_error().raw_os_error();
    if result == -1 && error == Some(libc::ESRCH) {
        return Ok(());
    }
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "pgid=,stat="])
        .output()?;
    anyhow::ensure!(output.status.success(), "Agent group identity unreadable");
    for line in String::from_utf8(output.stdout)?.lines() {
        let mut fields = line.split_whitespace();
        if fields.next().and_then(|field| field.parse::<u32>().ok()) == Some(pgid)
            && fields
                .next()
                .is_some_and(|state| !state.starts_with('Z') && !state.starts_with('X'))
        {
            anyhow::bail!("Agent process group {pgid} is alive");
        }
    }
    anyhow::ensure!(result == 0, "Agent process group unverifiable");
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn group_gone(_pgid: u32) -> Result<()> {
    anyhow::bail!("Agent group probe unavailable")
}

#[cfg(test)]
#[path = "execution_receipt_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "execution_barrier_tests.rs"]
mod barrier_tests;
