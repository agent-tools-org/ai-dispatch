// Caller-owned and JSON stop extend main's terminal-row behavior only.
// Signals verified worker/agent groups, then observes the existing settlement barrier.
use super::*;
use crate::cmd::execution_receipt;
use crate::store::execution_protocol::AgentProcess;
use crate::runtime::execution_probe as probe;

pub(super) fn action(store: &Arc<Store>, id: &str, graceful: bool, detail: &str, label: &str) -> Result<()> {
    let task = store
        .get_task(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    store.record_execution_stop(id)?;
    if !task.status.is_terminal() || (task.status == TaskStatus::Done && task.completed_at.is_none()) {
        store.settle_task_status(
            id,
            TaskStatus::Stopped,
            None,
            None,
            Some(&TaskEvent::new(
                task.id.clone(),
                EventKind::Completion,
                detail.into(),
            )),
        )?;
    }
    let spec = background::load_spec_if_exists(id)?;
    let (agent, spawned, _) = store.execution_process(id)?;
    let worker = spec.as_ref().and_then(|spec| spec.worker_pid);
    anyhow::ensure!(
        spec.as_ref().is_none_or(|spec| spec.task_id == id),
        "Worker spec identity mismatch"
    );
    anyhow::ensure!(
        !(spawned
            && spec
                .as_ref()
                .is_some_and(|spec| worker.is_none() && !spec.foreground)),
        "Worker identity missing"
    );
    anyhow::ensure!(
        !(spec.as_ref().is_some_and(|s| s.agent_pid.is_some())
            && agent.as_ref().is_none_or(|a| a.state == "not_started")),
        "Agent launch evidence missing"
    );
    if graceful && task.agent == crate::types::AgentKind::Codex {
        crate::runtime::codex_app_server::wait_for_interrupt(id);
    }
    let mut signalled = signal_agent(agent.as_ref(), !graceful)?;
    if let Some(pid) = worker {
        signalled |= signal_worker(pid, id, !graceful)?;
    }
    wait_gone(id, worker, agent.as_ref(), graceful)?;
    finish_stop(store, id, &task, label, signalled)
}

fn finish_stop(store: &Store, id: &str, task: &Task, label: &str, signalled: bool) -> Result<()> {
    if signalled {
        crate::runtime::sandbox::kill_container(id);
        preserve_worktree(id, &task, label);
        capture_final_worktree_state(store, &task.id)?;
        if let Some(path) = &task.worktree_path {
            crate::workspace::worktree::clear_worktree_lock(Path::new(path), id)?;
        }
    }
    // Never remove a live worker's barrier or replace it after a failed close.
    background::clear_spec(id)?;
    Ok(())
}

fn signal_worker(pid: u32, id: &str, force: bool) -> Result<bool> {
    if execution_receipt::worker_gone(pid, id)? {
        return Ok(false);
    }
    if force {
        background::sigkill_process(pid);
    } else {
        background::kill_process(pid);
    }
    Ok(true)
}

fn signal_agent(agent: Option<&AgentProcess>, force: bool) -> Result<bool> {
    let agent = agent.ok_or_else(|| anyhow!("No recorded agent group"))?;
    if agent.state == "not_started" {
        return Ok(false);
    }
    let (Some(pid), Some(pgid), Some(started)) = (agent.pid, agent.pgid, agent.started.as_deref()) else {
        bail!("Agent process group or launch identity missing");
    };
    anyhow::ensure!(
        pgid > 1 && pgid <= i32::MAX as u32 && !started.is_empty(),
        "Invalid agent identity"
    );
    if probe::alive(pid)? {
        let current = crate::runtime::process::monitor::activity::root_started(pid)
            .ok_or_else(|| anyhow!("Agent identity unreadable"))?;
        if current != started {
            return Ok(false);
        }
    }
    if execution_receipt::group_gone(pgid).is_ok() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        let result = unsafe { libc::kill(-(pgid as i32), signal) };
        anyhow::ensure!(
            result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
            "Agent group signal failed"
        );
    }
    Ok(true)
}

fn wait_gone(id: &str, worker: Option<u32>, agent: Option<&AgentProcess>, graceful: bool) -> Result<()> {
    let mut escalated = !graceful;
    let mut deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        let worker_gone = worker
            .map(|pid| execution_receipt::worker_gone(pid, id))
            .transpose()?
            .unwrap_or(true);
        if worker_gone && execution_receipt::agent_group_gone(agent).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            if escalated {
                bail!("Worker or agent group still alive or unverifiable");
            }
            if let Some(pid) = worker {
                signal_worker(pid, id, true)?;
            }
            signal_agent(agent, true)?;
            escalated = true;
            deadline = Instant::now() + WAIT_TIMEOUT;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

pub(super) fn text(
    store: &Arc<Store>,
    id: &str,
    graceful: bool,
    detail: &str,
    label: &str,
    print: Option<&str>,
) -> Result<()> {
    action(store, id, graceful, detail, label)?;
    let receipt = execution_receipt::receipt(store, id)?;
    anyhow::ensure!(receipt["settled"] == true, "Execution remains unconfirmed");
    if let Some(label) = print {
        println!("{label} {id}");
    }
    Ok(())
}
