// Execution receipts derive settlement from the existing job-spec barrier.
// Agent launch evidence survives spec removal in the task row; probes never reap processes.
use anyhow::Result;
use serde_json::{Value, json};
use crate::{
    runtime::{background, execution_probe as probe},
    store::Store,
};

pub(crate) fn receipt(store: &Store, id: &str) -> Result<Value> {
    if store.is_caller_owned(id)? { derive_settlement(store, id)?; }
    decorate(store, id, snapshot(store, id)?)
}

pub(crate) fn read_only_receipt(store: &Store, id: &str) -> Result<Value> {
    decorate(store, id, snapshot(store, id)?)
}

pub(crate) fn require_caller_owned(store: &Store, id: &str) -> Result<()> {
    let receipt = read_only_receipt(store, id)?;
    if receipt["policy"].is_null() {
        return Err(crate::execution::Refusal {
            code: "policy_unsupported", detail: Some("caller-owned".into()), exit: 3,
            receipt: Some(receipt),
        }.into());
    }
    Ok(())
}

fn snapshot(store: &Store, id: &str) -> Result<Value> {
    store
        .execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))
}

fn decorate(store: &Store, id: &str, mut receipt: Value) -> Result<Value> {
    if receipt["policy"].is_null() {
        receipt["agent_process"] = Value::Null;
    } else {
        receipt["agent_process"] = serde_json::to_value(store.execution_process(id)?.0)?;
    }
    receipt["settled"] = json!(receipt["worker"]["state"] == "settled");
    receipt["observed_at"] = json!(chrono::Utc::now().to_rfc3339());
    Ok(receipt)
}

pub(crate) fn derive_settlement(store: &Store, id: &str) -> Result<bool> {
    if !store.is_caller_owned(id)? { return Ok(false); }
    let facts = store
        .execution_facts(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    if facts.settlement.is_some_and(|worker| worker.state == "settled") {
        return Ok(false);
    }
    let worker = observation(store, id)?;
    if let Some(outcome) = &worker.outcome {
        return store.persist_settlement(id, outcome);
    }
    store.observe_worker_state(id, &worker.state, worker.pid, worker.detail)?;
    Ok(false)
}

fn observation(store: &Store, id: &str) -> Result<crate::store::execution_protocol::WorkerSettlement> {
    Ok(match observe(store, id) {
        Ok(worker) => worker,
        Err(error) => {
            let pid = store.execution_facts(id)?.and_then(|facts| facts.settlement)
                .and_then(|worker| worker.pid);
            worker("unconfirmed", pid, None, Some(error.to_string()))
        },
    })
}

fn worker(
    state: &str,
    pid: Option<u32>,
    outcome: Option<&str>,
    detail: Option<String>,
) -> crate::store::execution_protocol::WorkerSettlement {
    crate::store::execution_protocol::WorkerSettlement {
        state: state.into(),
        pid,
        outcome: outcome.map(str::to_owned),
        detail,
        settled_at: outcome.map(|_| chrono::Utc::now().to_rfc3339()),
    }
}

fn observe(store: &Store, id: &str) -> Result<crate::store::execution_protocol::WorkerSettlement> {
    let (spec, recorded_pid) = worker_spec(store, id)?;
    #[cfg(test)]
    OBSERVE_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    if let Some(spec) = &spec {
        anyhow::ensure!(spec.task_id == id, "Worker spec identity mismatch");
        if let Some(pid) = spec.worker_pid.or(recorded_pid) {
            if !worker_gone(pid, id)? {
                return Ok(worker("running", Some(pid), None, None));
            }
        } else {
            return Ok(worker("pending", None, None, None));
        }
    }
    if spec.is_none() && let Some(pid) = recorded_pid {
        if !worker_gone(pid, id)? { return Ok(worker("running", Some(pid), None, None)); }
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
    let spawned = spawned || recorded_pid.is_some()
        || spec.as_ref().is_some_and(|spec| spec.worker_pid.is_some());
    if !spawned
        && agent.as_ref().is_some_and(|agent| agent.state == "not_started")
        && !task.status.is_terminal()
    {
        return Ok(worker("pending", None, None, None));
    }
    let outcome = if stopped {
        "stopped"
    } else if !spawned && agent.as_ref().is_some_and(|agent| agent.state == "not_started") {
        "launch_failed"
    } else if crate::support::paths::job_path(id).try_exists()? {
        "lost"
    } else {
        "completed"
    };
    Ok(worker("settled", None, Some(outcome), None))
}

pub(crate) fn worker_spec(store: &Store, id: &str)
    -> Result<(Option<background::BackgroundRunSpec>, Option<u32>)> {
    let recorded_pid = store.execution_facts(id)?.and_then(|facts| facts.settlement)
        .and_then(|worker| worker.pid);
    let spec = match background::load_spec_if_exists(id) {
        Ok(spec) => spec,
        Err(_) if recorded_pid.is_some() => None,
        Err(error) => return Err(error),
    };
    Ok((spec, recorded_pid))
}

pub(crate) fn worker_gone(pid: u32, id: &str) -> Result<bool> {
    if !probe::alive(pid)? {
        return Ok(true);
    }
    let args = match probe::field(pid, "args") {
        Ok(args) => args,
        Err(_) if !probe::alive(pid)? => return Ok(true),
        Err(error) => return Err(error),
    };
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
    let (Some(_pid), Some(pgid), Some(started)) = (agent.pid, agent.pgid, agent.started.as_deref()) else {
        anyhow::bail!("Agent process group or launch identity missing");
    };
    anyhow::ensure!(
        pgid > 1 && pgid <= i32::MAX as u32 && !started.is_empty(),
        "Invalid agent identity"
    );
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

#[cfg(test)]
thread_local! {
    static OBSERVE_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
#[path = "execution_receipt_round6_tests.rs"]
mod round6_tests;

#[cfg(test)]
#[path = "execution_round8_tests.rs"]
mod round8_tests;
