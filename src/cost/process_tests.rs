// Completion recording through a real child-process stream using captured Codex JSONL.
// Covers component cost persistence and ceiling enforcement in the run process path.

use super::test_support::*;
use crate::agent::codex::CodexAgent;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskStatus};
use std::sync::Arc;

const CODEX_MESSAGE: &str = concat!(
    r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Plannin"#,
    r#"g the next edit."}}"#,
);

async fn run_capture(ceiling: f64) -> Task {
    crate::paths::ensure_dirs().expect("aid dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = task(AgentKind::Codex, 238_440, None);
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("task");
    let mut cmd = tokio::process::Command::new("sh");
    cmd.args([
        "-c",
        "printf '%s\\n%s\\n' \"$1\" \"$2\"",
        "capture",
        CODEX_USAGE,
        CODEX_MESSAGE,
    ]);
    crate::cmd::run::run_agent_process_with_cost(
        &CodexAgent,
        cmd,
        &task.id,
        &store,
        &crate::paths::log_path(task.id.as_str()),
        None,
        task.requested_model.as_deref(),
        true,
        None,
        crate::timeout_policy::TimeoutPolicy::default(),
        Some(ceiling),
    )
    .await
    .expect("run captured stream");
    store
        .get_task(task.id.as_str())
        .expect("query")
        .expect("stored task")
}

#[tokio::test]
async fn cost_process_codex_capture_stores_components_and_enforces_ceiling() {
    let _guard = seed_prices(Some(0.125));
    for (ceiling, status) in [(0.12, TaskStatus::Done), (0.10, TaskStatus::Failed)] {
        let stored = run_capture(ceiling).await;
        assert_cost(stored.cost_usd, 0.111981);
        assert_eq!(stored.status, status);
    }
}
