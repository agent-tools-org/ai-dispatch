// JSON settlement wait shares the existing worker completion barrier.
// Prints a receipt on both success and timeout, including a single observation at zero.
use super::*;

pub(crate) async fn run_json(store: &Store, ids: &[String], timeout: Option<u64>) -> Result<i32> {
    anyhow::ensure!(ids.len() == 1, "aid wait --json requires exactly one task ID");
    let id = &ids[0];
    wait_json_with(store, id, timeout, crate::cmd::execution_receipt::receipt).await
}

async fn wait_json_with(
    store: &Store, id: &str, timeout: Option<u64>, mut observe: impl FnMut(&Store, &str) -> Result<serde_json::Value>,
) -> Result<i32> {
    let start = Instant::now();
    let mut last = None;
    let mut error = None;
    loop {
        match observe(store, id) {
            Ok(receipt) if receipt["settled"] == true => { println!("{receipt}"); return Ok(0); }
            Ok(receipt) => last = Some(receipt),
            Err(tick) => {
                if tick.downcast_ref::<crate::execution::Refusal>().is_some_and(|refusal| refusal.code == "not_found") {
                    return Err(tick);
                }
                error = Some(tick);
            }
        }
        if timeout.is_some_and(|seconds| start.elapsed() >= Duration::from_secs(seconds)) {
            let receipt = last.ok_or_else(|| error.unwrap_or_else(|| anyhow::anyhow!("No receipt available")))?;
            println!("{receipt}");
            return Ok(124);
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
        store.db().execute("UPDATE tasks SET agent_process = NULL WHERE id = ?1", [task.id.as_str()]).expect("missing evidence");
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
    #[tokio::test]
    async fn caller_owned_unconfirmed_wait_remains_open_until_timeout() {
        let (_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
        let task = Task::test_fixture(crate::types::TaskId("t-wait-unconfirmed".into()), TaskStatus::Failed);
        store.insert_task(&task).expect("task");
        store.db().execute("UPDATE tasks SET agent_process = NULL WHERE id = ?1", [task.id.as_str()]).expect("missing evidence");
        let started = Instant::now();
        assert_eq!(run_json(&store, &[task.id.to_string()], Some(1)).await.expect("wait"), 124);
        assert!(started.elapsed() >= Duration::from_secs(1), "unconfirmed is not an early timeout");
    }

    #[tokio::test]
    async fn default_text_wait_keeps_main_terminal_behavior_without_group_evidence() {
        let (_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
        let mut task = Task::test_fixture(crate::types::TaskId("t-main-wait".into()), TaskStatus::Done);
        task.completed_at = Some(chrono::Local::now());
        store.insert_task(&task).expect("task");
        store.db().execute("UPDATE tasks SET agent_process = NULL WHERE id = ?1", [task.id.as_str()]).expect("legacy");
        let ids = [task.id.to_string()];
        assert_eq!(wait_for_task_ids(&store, &ids, None, false, Some(Duration::from_secs(1)))
            .await.expect("main text wait"), WaitOutcome::Completed);
        assert_eq!(run_json(&store, &ids, Some(0)).await.expect("JSON evidence wait"), 124);
    }

    #[tokio::test]
    async fn caller_owned_wait_recovers_after_transient_read_errors() {
        let store = Store::open_memory().expect("store");
        let mut tick = 0;
        let code = wait_json_with(&store,"t-transient",Some(1),|_,_| {
            tick += 1;
            match tick {
                1 | 3 => anyhow::bail!("transient store error"),
                2 => Ok(serde_json::json!({"settled":false})),
                _ => Ok(serde_json::json!({"settled":true})),
            }
        }).await.expect("recovered");
        assert_eq!(code,0);
        assert_eq!(tick,4);
    }

    #[tokio::test]
    async fn caller_owned_wait_prints_last_receipt_when_later_ticks_fail() {
        let store = Store::open_memory().expect("store");
        let mut first = true;
        let started = Instant::now();
        assert_eq!(wait_json_with(&store,"t-transient",Some(1),|_,_| {
            if first { first=false; Ok(serde_json::json!({"settled":false})) }
            else { anyhow::bail!("transient store error") }
        }).await.expect("timeout with receipt"),124);
        assert!(started.elapsed()>=Duration::from_secs(1));
    }

    #[tokio::test]
    async fn caller_owned_wait_reports_read_error_only_after_no_tick_ever_succeeds() {
        let store = Store::open_memory().expect("store");
        let started = Instant::now();
        assert!(wait_json_with(&store,"t-transient",Some(1),|_,_| anyhow::bail!("store unavailable")).await.is_err());
        assert!(started.elapsed()>=Duration::from_secs(1));
    }

}
