// Tests for `cmd::prompt_context` helpers.
// Exports: none.
// Deps: prompt_context internals, in-memory Store, tempfile.

use super::*;
use chrono::Local;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use tempfile::NamedTempFile;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
    Task {
        agent,
        prompt: "test prompt".to_string(),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

fn make_entry(topic: &str, path: Option<&str>, description: &str, content: Option<&str>) -> KnowledgeEntry {
    KnowledgeEntry {
        topic: topic.to_string(),
        path: path.map(str::to_string),
        description: description.to_string(),
        content: content.map(str::to_string),
    }
}

fn make_memory_with_age(id: &str, memory_type: MemoryType, content: &str, age: chrono::Duration) -> Memory {
    Memory {
        id: MemoryId(id.to_string()),
        memory_type,
        tier: MemoryTier::OnDemand,
        content: content.to_string(),
        source_task_id: None,
        agent: None,
        project_path: Some("/test-project".to_string()),
        content_hash: format!("hash-{id}"),
        created_at: Local::now() - age,
        expires_at: None,
        supersedes: None,
        version: 1,
        inject_count: 0,
        last_injected_at: None,
        success_count: 0,
    }
}

fn make_memory(id: &str, tier: MemoryTier, content: &str, project_path: Option<String>) -> Memory {
    Memory {
        id: MemoryId(id.to_string()),
        memory_type: MemoryType::Fact,
        tier,
        content: content.to_string(),
        source_task_id: None,
        agent: None,
        project_path,
        content_hash: format!("hash-{id}"),
        created_at: Local::now(),
        expires_at: None,
        supersedes: None,
        version: 1,
        inject_count: 0,
        last_injected_at: None,
        success_count: 0,
    }
}


#[path = "prompt_context_tests_selection.rs"]
mod selection;

#[path = "prompt_context_tests_injection.rs"]
mod injection;
