// Fleet and agent roster endpoints plus the shared summary calculation.
// Exports: `/api/fleet`, `/api/agents`, FleetSummary, and agent SSE helpers.
// Deps: Store batch queries, agent JSON builder, API DTOs, and axum.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::cmd::agent_json;
use crate::store::Store;
use crate::types::{Task, TaskFilter};

use super::api::{enrich_tasks, internal_error, task_memory_mb};
use super::api_types::{AgentResponse, TaskEnrichment, TaskResponse};

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub host: String,
    pub port: u16,
    pub started_at: String,
}

#[derive(Debug, Deserialize)]
pub struct FleetParams {
    pub window: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct FleetSummary {
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub stopped: usize,
    pub spend_usd: f64,
    pub tokens: i64,
    pub memory_mb: Option<i64>,
    pub window: String,
}

#[derive(Debug, Serialize)]
pub struct FleetResponse {
    pub server: ServerPayload,
    pub summary: FleetSummary,
    pub sectors: Vec<SectorResponse>,
    pub agents: Vec<AgentResponse>,
}

#[derive(Debug, Serialize)]
pub struct ServerPayload {
    pub version: String,
    pub host: String,
    pub port: u16,
    pub started_at: String,
    pub aid_home: String,
}

#[derive(Debug, Serialize)]
pub struct SectorResponse {
    pub id: String,
    pub name: String,
    pub repo_path: Option<String>,
    pub workgroup_id: Option<String>,
    pub tasks: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy)]
enum Window {
    Today,
    Hours24,
    Days7,
    Days30,
    All,
}

impl Window {
    fn parse(value: Option<&str>) -> Option<Self> {
        match value.unwrap_or("today") {
            "today" => Some(Self::Today),
            "24h" => Some(Self::Hours24),
            "7d" => Some(Self::Days7),
            "30d" => Some(Self::Days30),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::All => "all",
        }
    }

    fn includes(self, created_at: DateTime<Local>, now: DateTime<Local>) -> bool {
        match self {
            Self::Today => created_at.date_naive() == now.date_naive(),
            Self::Hours24 => created_at >= now - Duration::hours(24),
            Self::Days7 => created_at >= now - Duration::days(7),
            Self::Days30 => created_at >= now - Duration::days(30),
            Self::All => true,
        }
    }
}

pub async fn get_fleet(
    Query(params): Query<FleetParams>,
    State(store): State<Arc<Store>>,
    Extension(server): Extension<ServerInfo>,
) -> Result<Json<FleetResponse>, StatusCode> {
    let window = Window::parse(params.window.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
    let now = Local::now();
    let tasks = store
        .list_tasks(TaskFilter::All)
        .map_err(internal_error)?
        .into_iter()
        .filter(|task| window.includes(task.created_at, now))
        .collect::<Vec<_>>();
    let responses = enrich_tasks(&store, tasks).map_err(internal_error)?;
    let summary = summary_for_responses(&responses, window.label());
    let sectors = build_sectors(responses).map_err(internal_error)?;
    let running = store.list_tasks(TaskFilter::Running).map_err(internal_error)?;
    let agents = build_agents(&store, &running).map_err(internal_error)?;
    Ok(Json(FleetResponse {
        server: ServerPayload {
            version: env!("CARGO_PKG_VERSION").to_string(),
            host: server.host,
            port: server.port,
            started_at: server.started_at,
            aid_home: crate::paths::aid_dir().display().to_string(),
        },
        summary,
        sectors,
        agents,
    }))
}

pub async fn get_agents(State(store): State<Arc<Store>>) -> Result<Json<Vec<AgentResponse>>, StatusCode> {
    let running = store.list_tasks(TaskFilter::Running).map_err(internal_error)?;
    Ok(Json(build_agents(&store, &running).map_err(internal_error)?))
}

pub(crate) fn build_agents(store: &Store, running: &[Task]) -> anyhow::Result<Vec<AgentResponse>> {
    let list = agent_json::get_agents_list(store)?;
    Ok(list
        .agents
        .into_iter()
        .map(|agent| {
            let running_task_ids = running
                .iter()
                .filter(|task| agent_matches_task(&agent.name, &agent.kind, task))
                .map(|task| task.id.to_string())
                .collect();
            AgentResponse::from_json(agent, running_task_ids)
        })
        .collect())
}

pub(crate) fn summary_for_tasks(tasks: &[Task], window: &str) -> FleetSummary {
    let responses = tasks.iter().cloned().map(|task| {
        let memory_mb = task_memory_mb(&task);
        TaskResponse::from_task(task, TaskEnrichment { memory_mb, ..Default::default() })
    }).collect::<Vec<_>>();
    summary_for_responses(&responses, window)
}

fn summary_for_responses(tasks: &[TaskResponse], window: &str) -> FleetSummary {
    let mut summary = FleetSummary {
        running: 0,
        done: 0,
        failed: 0,
        stopped: 0,
        spend_usd: 0.0,
        tokens: 0,
        memory_mb: None,
        window: window.to_string(),
    };
    for task in tasks {
        match task.status.as_str() {
            "running" | "awaiting_input" | "stalled" => summary.running += 1,
            "done" | "merged" => summary.done += 1,
            "failed" => summary.failed += 1,
            "stopped" => summary.stopped += 1,
            _ => {}
        }
        summary.spend_usd += task.cost_usd.unwrap_or(0.0);
        summary.tokens += task.tokens.unwrap_or(0);
        if let Some(memory_mb) = task.memory_mb {
            summary.memory_mb = Some(summary.memory_mb.unwrap_or(0) + memory_mb);
        }
    }
    summary
}

fn build_sectors(tasks: Vec<TaskResponse>) -> anyhow::Result<Vec<SectorResponse>> {
    let mut sectors: HashMap<String, SectorResponse> = HashMap::new();
    for task in tasks {
        let id = task.sector_id.clone().unwrap_or_else(|| "unassigned".to_string());
        let value = serde_json::to_value(&task)?;
        let mut value = value;
        if let Some(object) = value.as_object_mut() {
            object.remove("prompt");
            object.remove("resolved_prompt");
        }
        let sector = sectors.entry(id.clone()).or_insert_with(|| SectorResponse {
            id: id.clone(),
            name: id,
            repo_path: task.repo_path.clone(),
            workgroup_id: task.workgroup_id.clone(),
            tasks: Vec::new(),
        });
        if sector.workgroup_id.is_none() {
            sector.workgroup_id = task.workgroup_id.clone();
        }
        sector.tasks.push(value);
    }
    let mut sectors: Vec<_> = sectors.into_values().collect();
    sectors.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sectors)
}

fn agent_matches_task(name: &str, kind: &str, task: &Task) -> bool {
    if kind == "custom" {
        return task.agent == crate::types::AgentKind::Custom
            && task.custom_agent_name.as_deref() == Some(name);
    }
    task.agent.as_str() == name
}
