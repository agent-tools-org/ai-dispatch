// JSON stop receipts report which closer won settlement.
// Uses the same cleanup and confirmation path as text stop.
use super::*;
pub(crate) fn stop_json(store: &Arc<Store>, id: &str, force: bool) -> Result<i32> {
    let before = store
        .execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    let _ = before;
    let result = terminate_task(store, id, !force, "Task stopped by user", "stopped", true);
    if let Err(error) = &result {
        let pid = background::load_spec_if_exists(id)
            .ok()
            .flatten()
            .and_then(|spec| spec.worker_pid.or(spec.agent_pid));
        store.observe_worker_state(id, "unconfirmed", pid, Some(error.to_string()))?;
    }
    let mut receipt = store
        .execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    receipt["settled"] = serde_json::json!(receipt["worker"]["state"] == "settled");
    receipt["observed_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    let other_settlement = receipt["settled"] == true && !matches!(&result, Ok((_, Closed::Written)));
    let (outcome, detail, code) = match result {
        _ if other_settlement => ("already_settled", None, 0),
        Ok((_, closed)) => (stop_result(&closed), None, 0),
        Err(error) => ("unconfirmed", Some(error.to_string()), 5),
    };
    receipt["stop"] = serde_json::json!({"result": outcome, "detail": detail});
    println!("{receipt}");
    Ok(code)
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn caller_owned_stop_json_is_idempotent_and_preserves_existing_settlement() {
        let home = tempfile::tempdir().expect("home");
        let _guard = crate::support::paths::AidHomeGuard::set(home.path());
        let store = Arc::new(Store::open_memory().expect("store"));
        let mut task = tests::make_task("t-stop-json", TaskStatus::Done);
        task.completed_at = Some(chrono::Local::now());
        store.insert_task(&task).expect("task");
        assert_eq!(stop_json(&store, task.id.as_str(), false).expect("stop"), 0);
        let before = store.execution_snapshot(task.id.as_str()).expect("snapshot");
        assert_eq!(
            stop_json(&store, task.id.as_str(), true).expect("already settled"),
            0
        );
        assert_eq!(
            store.execution_snapshot(task.id.as_str()).expect("snapshot"),
            before
        );
        assert_eq!(
            store
                .execution_facts(task.id.as_str())
                .expect("facts")
                .expect("row")
                .settlement
                .expect("worker")
                .outcome
                .as_deref(),
            Some("stopped")
        );
    }
}

pub(super) fn stop_result(closed: &Closed) -> &'static str {
    if *closed == Closed::Existing {
        "already_settled"
    } else {
        "stopped"
    }
}
