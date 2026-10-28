// Store tests module wiring and shared helpers.
// Exports: test submodules and make_task helper.
// Deps: Store, chrono.

use chrono::Local;

use super::*;
use crate::types::*;

fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
    Task {
        agent,
        prompt: "test prompt".to_string(),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

mod agent_tests;
mod db_tests;
mod effective_dir_migration_tests;
mod event_tests;
mod kg_tests;
mod task_tests;
mod workgroup_tests;
mod memory_tests;
mod status_guard_tests;
