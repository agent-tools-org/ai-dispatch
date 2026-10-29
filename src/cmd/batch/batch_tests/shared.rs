// Shared test helpers for batch submodule tests.
// Exports: make_task, seed_task, make_stored_task
// Deps: crate::{batch,store,types}
use crate::batch;
use crate::store::Store;
use crate::types::Task;
use crate::types::{AgentKind, TaskId, TaskStatus};
use chrono::Local;

pub(super) fn make_task(name: &str, conditional: bool, on_success: Option<&str>) -> batch::BatchTask {
    batch::BatchTask {
        name: Some(name.to_string()),
        agent: "codex".to_string(),
        prompt: "prompt".to_string(),
        on_success: on_success.map(str::to_string),
        conditional,
        ..Default::default()
    }
}

pub(super) fn seed_task(store: &Store, task_id: &str, status: TaskStatus, cost_usd: Option<f64>) {
    let mut task = make_stored_task(task_id, AgentKind::Codex, status);
    task.duration_ms = Some(1_000);
    task.requested_model = Some("gpt-5".to_string());
    task.cost_usd = cost_usd;
    task.completed_at = Some(Local::now());
    store.insert_task(&task).unwrap();
}

pub(super) fn make_stored_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
    Task {
        agent,
        prompt: "test prompt".to_string(),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}
