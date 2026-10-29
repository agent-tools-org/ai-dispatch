// The sole production settlement writer for aid.execution/1.
// Callers finish cleanup first; unverifiable evidence keeps the job barrier available.
use anyhow::Result;
use crate::store::{Store, execution_protocol::WorkerSettlement};
use super::{
    background::{self, BackgroundRunSpec},
    execution_processes,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closer {
    Worker,
    Stop,
    Reaper,
    Observation,
    LaunchFailed,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Closed {
    Written,
    Existing,
    Unconfirmed(String),
}

pub(crate) fn close_execution(
    store: &Store,
    id: &str,
    outcome: &str,
    caller: Closer,
    spec: Option<&BackgroundRunSpec>,
) -> Result<Closed> {
    if store
        .execution_facts(id)?
        .and_then(|f| f.settlement)
        .is_some_and(|s| s.state == "settled")
    {
        return Ok(Closed::Existing);
    }
    let evidence = spec
        .cloned()
        .map(Some)
        .map(Ok)
        .unwrap_or_else(|| background::load_spec_if_exists(id));
    let mut evidence = match evidence {
        Ok(spec) => spec,
        Err(error) => return Ok(Closed::Unconfirmed(error.to_string())),
    };
    if caller == Closer::Observation
        && let Some(spec) = evidence.as_mut()
    {
        execution_processes::snapshot(spec);
    }
    if let Err(error) = confirm_exit(store, id, caller, evidence.as_ref()) {
        if let Some(spec) = evidence.as_ref() {
            background::save_spec(spec)?;
        }
        return Ok(Closed::Unconfirmed(error.to_string()));
    }
    anyhow::ensure!(
        matches!(outcome, "completed" | "stopped" | "lost" | "launch_failed"),
        "Invalid outcome"
    );
    anyhow::ensure!(
        outcome != "launch_failed" || caller == Closer::LaunchFailed,
        "Worker was spawned"
    );
    persist_or_keep(store, id, outcome, evidence.as_ref())
}

fn persist_or_keep(
    store: &Store,
    id: &str,
    outcome: &str,
    spec: Option<&BackgroundRunSpec>,
) -> Result<Closed> {
    let result = persist(store, id, outcome);
    if result.is_err()
        && let Some(spec) = spec
    {
        if let Err(error) = background::save_spec(spec) {
            aid_warn!("[aid] Could not retain execution evidence for {id}: {error}");
        }
    }
    result
}

fn persist(store: &Store, id: &str, outcome: &str) -> Result<Closed> {
    let settlement = WorkerSettlement {
        state: "settled".into(),
        outcome: Some(outcome.into()),
        pid: None,
        settled_at: Some(chrono::Utc::now().to_rfc3339()),
        detail: None,
    };
    let changed = store.db().execute(
        "UPDATE tasks SET worker_settlement = ?2 WHERE id = ?1
         AND COALESCE(json_extract(worker_settlement, '$.state'), '') != 'settled'",
        rusqlite::params![id, serde_json::to_string(&settlement)?],
    )?;
    Ok(if changed == 1 {
        Closed::Written
    } else {
        Closed::Existing
    })
}

fn confirm_exit(store: &Store, id: &str, caller: Closer, spec: Option<&BackgroundRunSpec>) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match confirm(store, id, caller, spec) {
            Err(error)
                if matches!(caller, Closer::Stop | Closer::Reaper)
                    && error.to_string().contains("is alive")
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            result => return result,
        }
    }
}

fn confirm(store: &Store, id: &str, caller: Closer, spec: Option<&BackgroundRunSpec>) -> Result<()> {
    if let Some(spec) = spec {
        anyhow::ensure!(spec.task_id == id, "Task identity mismatch");
        if caller == Closer::LaunchFailed {
            anyhow::ensure!(spec.worker_pid.is_none(), "Worker was spawned");
            anyhow::ensure!(
                store
                    .execution_facts(id)?
                    .and_then(|f| f.settlement)
                    .and_then(|s| s.pid)
                    .is_none(),
                "Worker was spawned"
            );
        }
        execution_processes::processes_gone(spec, caller == Closer::Worker)?;
    } else {
        let worker = store.execution_facts(id)?.and_then(|f| f.settlement);
        anyhow::ensure!(
            worker.as_ref().and_then(|w| w.pid).is_none(),
            "No worker identity evidence"
        );
        anyhow::ensure!(caller != Closer::Observation, "No worker evidence");
    }
    Ok(())
}

pub(crate) fn close_warn(
    store: &Store,
    id: &str,
    outcome: &str,
    caller: Closer,
    spec: Option<&BackgroundRunSpec>,
) {
    match close_execution(store, id, outcome, caller, spec) {
        Ok(Closed::Unconfirmed(reason)) => aid_warn!("[aid] Execution {id} remains unconfirmed: {reason}"),
        Err(error) => {
            aid_warn!("[aid] Could not close execution {id}: {error}");
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "execution_close_tests.rs"]
mod tests;
