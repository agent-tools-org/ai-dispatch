// JSON stop uses the extended signal path, then derives settlement by observation.
// Default text stop remains main's implementation.
use super::*;

pub(crate) fn stop_json(store: &Arc<Store>, id: &str, force: bool) -> Result<i32> {
    crate::cmd::execution_receipt::require_caller_owned(store, id)?;
    let before = store
        .execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    if before["worker"]["state"] == "settled" {
        let mut receipt = crate::cmd::execution_receipt::receipt(store, id)?;
        receipt["stop"] = serde_json::json!({"result":"already_settled","detail":null});
        println!("{receipt}");
        return Ok(0);
    }
    let result = protocol::action(store, id, !force, "Task stopped by user", "stopped");
    let (mut receipt, code, label, detail) = match result {
        Ok(()) => {
            let written = crate::cmd::execution_receipt::derive_settlement(store, id)?;
            let receipt = crate::cmd::execution_receipt::receipt(store, id)?;
            if receipt["settled"] == true {
                (
                    receipt,
                    0,
                    if written { "stopped" } else { "already_settled" },
                    None,
                )
            } else {
                (
                    receipt,
                    5,
                    "unconfirmed",
                    Some("Execution remains unconfirmed".into()),
                )
            }
        }
        Err(error) => {
            record_unconfirmed(store, id, &error)?;
            let mut receipt = store
                .execution_snapshot(id)?
                .ok_or_else(|| anyhow!("Task disappeared"))?;
            receipt["settled"] = serde_json::json!(receipt["worker"]["state"] == "settled");
            receipt["observed_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
            (receipt, 5, "unconfirmed", Some(error.to_string()))
        }
    };
    receipt["stop"] = serde_json::json!({"result":label,"detail":detail});
    println!("{receipt}");
    Ok(code)
}

fn record_unconfirmed(store: &Store, id: &str, error: &anyhow::Error) -> Result<()> {
            let pid = background::load_spec_if_exists(id)
                .ok()
                .flatten()
                .and_then(|s| s.worker_pid)
                .or(store.execution_facts(id)?.and_then(|facts| facts.settlement)
                    .and_then(|worker| worker.pid));
            store.observe_worker_state(id, "unconfirmed", pid, Some(error.to_string()))?;
    Ok(())
}
