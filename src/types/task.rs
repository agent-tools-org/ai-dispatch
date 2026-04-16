// Task-centric domain structs for aid task storage and display.
// Exports: Task, Workgroup, Finding, TaskEvent, TaskFilter, CompletionInfo.
// Deps: chrono, serde, and parent `crate::types` enums/IDs.

use chrono::{DateTime, Local};
use serde::Serialize;

use super::{
    AgentKind, DeliveryAssessment, EventKind, TaskId, TaskStatus, VerifyStatus, WorkgroupId,
};

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: TaskId,
    pub agent: AgentKind,
    pub custom_agent_name: Option<String>,
    pub prompt: String,
    pub resolved_prompt: Option<String>,
    pub category: Option<String>,
    pub status: TaskStatus,
    pub parent_task_id: Option<String>,
    pub workgroup_id: Option<String>,
    pub caller_kind: Option<String>,
    pub caller_session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub start_sha: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub verify: Option<String>,
    pub verify_status: VerifyStatus,
    pub pending_reason: Option<String>,
    pub read_only: bool,
    pub budget: bool,
    pub audit_verdict: Option<String>,
    pub audit_report_path: Option<String>,
}

impl Task {
    pub fn agent_display_name(&self) -> &str {
        if self.agent == AgentKind::Custom {
            self.custom_agent_name.as_deref().unwrap_or("custom")
        } else {
            self.agent.as_str()
        }
    }

    pub fn delivery_assessment(&self) -> Option<DeliveryAssessment> {
        DeliveryAssessment::from_verify_status(self.verify_status)
    }

    pub fn has_verify_failure(&self) -> bool {
        self.verify_status == VerifyStatus::Failed
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Workgroup {
    pub id: WorkgroupId,
    pub name: String,
    pub shared_context: String,
    pub created_by: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: i64,
    pub workgroup_id: String,
    pub content: String,
    pub source_task_id: Option<String>,
    pub severity: Option<String>,
    pub title: Option<String>,
    pub file: Option<String>,
    pub lines: Option<String>,
    pub category: Option<String>,
    pub confidence: Option<String>,
    pub verdict: Option<String>,
    pub score: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskEvent {
    pub task_id: TaskId,
    pub timestamp: DateTime<Local>,
    pub event_kind: EventKind,
    pub detail: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy)]
pub enum TaskFilter {
    All,
    Active,
    Running,
    Today,
}

#[derive(Debug, Clone)]
pub struct CompletionInfo {
    pub tokens: Option<i64>,
    pub status: TaskStatus,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
}
