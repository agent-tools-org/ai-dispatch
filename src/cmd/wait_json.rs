// JSON settlement wait shares the existing worker completion barrier.
// Prints a receipt on both success and timeout, including a single observation at zero.
use super::*;

pub(crate) async fn run_json(store: &Store, ids: &[String], timeout: Option<u64>) -> Result<i32> {
    anyhow::ensure!(ids.len() == 1, "aid wait --json requires exactly one task ID");
    let id = &ids[0];
    let start = Instant::now();
    loop {
        let barrier = worker_is_settling(id);
        let receipt = crate::cmd::execution_receipt::receipt(store, id)?;
        if receipt["settled"] == true {
            println!("{receipt}");
            return Ok(0);
        }
        let unconfirmed_without_live =
            receipt["worker"]["state"] == "unconfirmed" && receipt["worker"]["pid"].is_null();
        if unconfirmed_without_live
            || timeout.is_some_and(|seconds| start.elapsed() >= Duration::from_secs(seconds))
        {
            println!("{receipt}");
            return Ok(124);
        }
        // The same barrier read used by ordinary wait owns completion ordering.
        if let Err(error) = barrier {
            if receipt["worker"]["state"] != "unconfirmed" {
                return Err(error);
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caller_owned_wait_zero_observes_settlement_independently_of_terminal_status() {
        let home = tempfile::tempdir().expect("home");
        let _guard = crate::support::paths::AidHomeGuard::set(home.path());
        let store = Store::open_memory().expect("store");
        let mut task = crate::cost::test_support::task(crate::types::AgentKind::Codex, 0, None);
        task.id = crate::types::TaskId("t-wait-json".into());
        task.status = TaskStatus::Failed;
        store.insert_task(&task).expect("task");
        let ids = vec![task.id.0.clone()];
        assert_eq!(run_json(&store, &ids, Some(0)).await.expect("observe"), 124);
        store
            .settle_worker(task.id.as_str(), "completed", None)
            .expect("settled");
        assert_eq!(
            run_json(&store, &ids, Some(0))
                .await
                .expect("settled observation"),
            0
        );
        let error = run_json(&store, &["t-missing".into()], Some(0))
            .await
            .expect_err("missing");
        assert_eq!(
            error
                .downcast_ref::<crate::execution::Refusal>()
                .expect("refusal")
                .exit,
            1
        );
        assert!(run_json(&store, &[], Some(0)).await.is_err());
    }
}
