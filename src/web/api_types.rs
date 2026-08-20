// JSON contracts shared by the web API handlers, fleet snapshot, and SSE.
// Exports: task, action, fleet, agent, and summary response DTOs.
// Deps: serde, crate::cmd::agent_json, crate::types, and Store enrichment.

use crate::cmd::agent_json_types::{AgentJson, QuotaJson};
use crate::types::{Task, TaskEvent, TaskProfileDeclaration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TaskListParams {
    pub filter: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TaskResponse {
    pub id: String,
    pub agent: String,
    pub custom_agent_name: Option<String>,
    pub prompt: String,
    pub resolved_prompt: Option<String>,
    pub status: String,
    pub outcome: String,
    pub parent_task_id: Option<String>,
    pub workgroup_id: Option<String>,
    pub caller_kind: Option<String>,
    pub caller_session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub repo_path: Option<String>,
    pub project_id: Option<String>,
    pub worktree_path: Option<String>,
    pub effective_dir: Option<String>,
    pub worktree_branch: Option<String>,
    pub final_head_sha: Option<String>,
    pub final_branch: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub requested_model: Option<String>,
    pub observed_model: Option<String>,
    pub attribution_source: Option<String>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub started_at: Option<String>,
    pub verify: Option<String>,
    pub verify_status: String,
    pub pending_reason: Option<String>,
    pub delivery_assessment: Option<String>,
    pub read_only: bool,
    pub budget: bool,
    pub prompt_excerpt: String,
    pub sector_id: Option<String>,
    pub difficulty: Option<String>,
    pub rigor: Option<String>,
    pub budget_class: Option<String>,
    pub urgency: Option<String>,
    pub memory_mb: Option<i64>,
    pub has_result: bool,
    pub has_diff: bool,
    pub awaiting_reason: Option<String>,
    pub latest_events: Vec<TaskEventResponse>,
    pub latest_milestone: Option<String>,
    pub latest_error: Option<String>,
}

#[derive(Debug, Default)]
pub struct TaskEnrichment {
    pub started_at: Option<String>,
    pub profile: TaskProfileDeclaration,
    pub latest_milestone: Option<String>,
    pub latest_error: Option<String>,
    pub awaiting_reason: Option<String>,
    pub latest_events: Vec<TaskEventResponse>,
    pub has_diff: bool,
}

impl TaskResponse {
    pub(crate) fn from_task(task: Task, enrichment: TaskEnrichment) -> Self {
        let outcome = task.outcome().as_str().to_string();
        let sector_id = task_sector_id(&task);
        Self {
            id: task.id.to_string(), agent: task.agent.as_str().to_string(), custom_agent_name: task.custom_agent_name,
            prompt_excerpt: task.prompt.chars().take(160).collect(), prompt: task.prompt, resolved_prompt: task.resolved_prompt,
            status: task.status.as_str().to_string(), outcome, parent_task_id: task.parent_task_id, workgroup_id: task.workgroup_id,
            caller_kind: task.caller_kind, caller_session_id: task.caller_session_id, agent_session_id: task.agent_session_id,
            repo_path: task.repo_path, project_id: task.project_id, worktree_path: task.worktree_path,
            effective_dir: task.effective_dir, worktree_branch: task.worktree_branch, final_head_sha: task.final_head_sha,
            final_branch: task.final_branch, log_path: task.log_path, output_path: task.output_path,
            tokens: task.tokens, prompt_tokens: task.prompt_tokens, duration_ms: task.duration_ms,
            requested_model: task.requested_model, observed_model: task.observed_model,
            attribution_source: task.attribution_source.map(|value| value.as_str().to_string()),
            cost_usd: task.cost_usd, exit_code: task.exit_code, created_at: task.created_at.to_rfc3339(),
            completed_at: task.completed_at.map(|value| value.to_rfc3339()), started_at: enrichment.started_at,
            verify: task.verify, verify_status: task.verify_status.as_str().to_string(), pending_reason: task.pending_reason,
            delivery_assessment: task.delivery_assessment.map(|value| value.as_str().to_string()),
            read_only: task.read_only, budget: task.budget, sector_id,
            difficulty: enrichment.profile.difficulty.map(|value| value.label().to_string()), rigor: enrichment.profile.rigor.map(|value| value.label().to_string()),
            budget_class: enrichment.profile.budget.map(|value| value.label().to_string()), urgency: enrichment.profile.urgency.map(|value| value.label().to_string()),
            memory_mb: None, has_result: crate::paths::task_dir(task.id.as_str()).join("result.md").is_file(), has_diff: enrichment.has_diff,
            awaiting_reason: (task.status == crate::types::TaskStatus::AwaitingInput)
                .then_some(enrichment.awaiting_reason)
                .flatten(),
            latest_events: enrichment.latest_events, latest_milestone: enrichment.latest_milestone, latest_error: enrichment.latest_error,
        }
    }
}

fn task_sector_id(task: &Task) -> Option<String> {
    task.project_id.clone().or_else(|| {
        task.repo_path.as_deref().and_then(|path| {
            std::path::Path::new(path).file_name().and_then(|name| name.to_str()).map(str::to_string)
        })
    })
}

#[derive(Debug, Serialize, Clone)]
pub struct TaskEventResponse {
    pub task_id: String,
    pub timestamp: String,
    pub event_kind: String,
    pub detail: String,
    pub metadata: Option<serde_json::Value>,
}

impl From<TaskEvent> for TaskEventResponse {
    fn from(event: TaskEvent) -> Self {
        Self {
            task_id: event.task_id.to_string(),
            timestamp: event.timestamp.to_rfc3339(),
            event_kind: event.event_kind.as_str().to_string(),
            detail: event.detail,
            metadata: event.metadata,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TaskOutputResponse {
    pub output: String,
}

#[derive(Debug, Serialize)]
pub struct ResultResponse {
    pub result: String,
}

#[derive(Debug, Serialize)]
pub struct AgentUsageResponse {
    pub agent: String,
    pub success_rate: Option<f64>,
    pub task_count: usize,
    pub avg_cost: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub agents: Vec<AgentUsageResponse>,
}

#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub diff: String,
}

#[derive(Debug, Deserialize)]
pub struct RetryRequest {
    pub feedback: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRequest {
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AgentResponse {
    pub name: String,
    pub kind: String,
    pub installed: bool,
    pub disabled: bool,
    pub provider: String,
    pub metering: String,
    pub quota: QuotaJson,
    pub default_model: Option<String>,
    pub observed_model: Option<String>,
    pub busy: bool,
    pub running_task_ids: Vec<String>,
    pub success_rate: Option<f64>,
    pub task_count: u64,
    pub avg_cost_usd: Option<f64>,
}

impl AgentResponse {
    pub(crate) fn from_json(agent: AgentJson, running_task_ids: Vec<String>) -> Self {
        let history = agent.history.as_ref();
        Self {
            name: agent.name,
            kind: agent.kind,
            installed: agent.installed,
            disabled: agent.disabled,
            provider: agent.provider,
            metering: agent.metering,
            quota: agent.quota,
            default_model: agent.models.default,
            observed_model: None,
            busy: !running_task_ids.is_empty(),
            running_task_ids,
            success_rate: history.map(|value| value.success_rate),
            task_count: history.map(|value| value.tasks).unwrap_or(0),
            avg_cost_usd: history.and_then(|value| value.avg_cost_usd),
        }
    }
}
