// Execution receipts combine stored lifecycle facts with the existing worker barrier.
// No separate process ledger: job specs remain the worker identity evidence.
use anyhow::Result;
use serde_json::{Value, json};
use crate::{store::Store, runtime::background};

pub(crate) fn receipt(store: &Store, id: &str) -> Result<Value> {
    observe_worker(store, id)?;
    let mut receipt = store
        .execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    receipt["settled"] = json!(receipt["worker"]["state"] == "settled");
    receipt["observed_at"] = json!(chrono::Utc::now().to_rfc3339());
    Ok(receipt)
}

fn observe_worker(store: &Store, id: &str) -> Result<()> {
    let facts = store
        .execution_facts(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    anyhow::ensure!(facts.revision > 0, "Invalid execution revision for task {id}");
    if facts
        .settlement
        .as_ref()
        .is_some_and(|worker| worker.state == "settled")
    {
        return Ok(());
    }
    let stored_pid = facts
        .settlement
        .as_ref()
        .and_then(|worker| worker.pid)
        .filter(|pid| background::is_process_running(*pid));
    match background::load_spec_if_exists(id) {
        Ok(Some(spec)) => return observe_spec(store, id, &spec),
        Err(error) => return record(store, id, "unconfirmed", stored_pid, Some(error.to_string())),
        Ok(None) => {}
    }
    if facts
        .settlement
        .as_ref()
        .is_some_and(|worker| worker.state == "pending")
        && store
            .execution_snapshot(id)?
            .is_some_and(|receipt| receipt["task"]["terminal"] == false)
    {
        return Ok(());
    }
    record(
        store,
        id,
        "unconfirmed",
        stored_pid,
        Some("no worker evidence".into()),
    )
}

fn observe_spec(store: &Store, id: &str, spec: &background::BackgroundRunSpec) -> Result<()> {
    let Some(pid) = spec.worker_pid else {
        return record(store, id, "pending", None, None);
    };
    if !background::is_process_running(pid) {
        store.settle_worker(id, "lost", Some("worker exited before lifecycle settlement"))?;
        return Ok(());
    }
    if background::BackgroundRunSpec::is_task_worker(pid, id) {
        return record(store, id, "running", Some(pid), None);
    }
    record(
        store,
        id,
        "unconfirmed",
        Some(pid),
        Some("worker identity unverifiable".into()),
    )
}

fn record(store: &Store, id: &str, state: &str, pid: Option<u32>, detail: Option<String>) -> Result<()> {
    store.observe_worker_state(id, state, pid, detail)
}

#[cfg(test)]
#[path = "execution_receipt_tests.rs"]
mod tests;
