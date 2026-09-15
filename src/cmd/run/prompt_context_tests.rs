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
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "test prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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
