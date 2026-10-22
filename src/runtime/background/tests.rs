// Tests for background worker persistence and zombie-task cleanup.
// Covers spec serialization and store reconciliation side effects.

use super::*;
use chrono::{Duration, Local};

use super::{
    BackgroundRunSpec, ZOMBIE_FAILURE_DETAIL, build_on_done_command, check_zombie_tasks_with,
    cleanup_stale_pending_tasks, fail_stale_pending_task, kill, reaper, save_spec, waiting,
};
use crate::store::Store;
use crate::support::paths;
use crate::support::test_subprocess;
use crate::types::{
    AgentKind, EventKind, PendingReason, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus,
};

fn make_spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(if task_id == "t-1a1a" { 101 } else { 202 }),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        result_file: None,
        result_file_required: None,
        model: None,
        budget: false,
        session_id: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        judge_retry: false,
        max_duration_mins: None,
        max_duration_secs: None,
        max_task_cost: None,
        idle_timeout_secs: None,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        hooks: vec![],
        template: None,
        worktree: None,
        base_branch: None,
        peer_review: None,
        audit: false,
        audit_explicit: false,
        no_audit: false,
        scope: vec![],
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        agent_started: None,
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
        foreground: false,
    }
}

fn make_task(task_id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
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

fn insert_event(store: &Store, task_id: &str, age_secs: i64) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - Duration::seconds(age_secs),
            event_kind: EventKind::Milestone,
            detail: "last progress".to_string(),
            metadata: None,
        })
        .unwrap();
}

fn insert_idle_escalation_event(store: &Store, task_id: &str, age_secs: i64) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - Duration::seconds(age_secs),
            event_kind: EventKind::Milestone,
            detail: "Auto-escalated: task stalled".to_string(),
            metadata: Some(serde_json::json!({ "auto_escalated": true })),
        })
        .unwrap();
}

#[path = "spec_persistence_tests.rs"]
mod spec_persistence_tests;

#[path = "first_token_tests.rs"]
mod first_token_tests;

#[path = "pending_cleanup_tests.rs"]
mod pending_cleanup_tests;

#[path = "worker_cleanup_tests.rs"]
mod worker_cleanup_tests;

#[path = "reaper_policy_tests.rs"]
mod reaper_policy_tests;
