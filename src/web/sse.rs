// SSE endpoint for task status updates.
// Exports: sse_handler.
// Deps: axum SSE types, futures stream helpers, Store task queries.

use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};
use anyhow::Result;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, Stream};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Interval, MissedTickBehavior, interval};

use super::api_types::AgentResponse;
use super::fleet::{self, FleetSummary};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
const TASK_UPDATE_EVENT: &str = "task_update";
const HEARTBEAT_EVENT: &str = "heartbeat";
const AGENT_UPDATE_EVENT: &str = "agent_update";
const FLEET_SUMMARY_EVENT: &str = "fleet_summary";

#[derive(Debug, Serialize)]
struct TaskUpdatePayload {
    id: String,
    status: String,
    agent: String,
    tokens: Option<i64>,
    cost_usd: Option<f64>,
    duration_ms: Option<i64>,
    milestone: Option<String>,
    outcome: String,
    verify_status: String,
    sector_id: Option<String>,
    latest_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct HeartbeatPayload {
    timestamp: String,
}

#[derive(Debug, Serialize)]
struct AgentUpdatePayload {
    name: String,
    quota: crate::cmd::agent_json_types::QuotaJson,
    busy: bool,
    running_task_ids: Vec<String>,
}

struct SseState {
    store: Arc<Store>,
    pending: VecDeque<Event>,
    last_seen: HashMap<String, TaskStatus>,
    last_agents: HashMap<String, (String, Vec<String>)>,
    poll: Interval,
    heartbeat: Interval,
}

pub(crate) fn sse_handler(
    State(store): State<Arc<Store>>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let mut poll = interval(POLL_INTERVAL);
    poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut heartbeat = interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let stream = stream::unfold(
        SseState {
            store,
            pending: VecDeque::new(),
            last_seen: HashMap::new(),
            last_agents: HashMap::new(),
            poll,
            heartbeat,
        },
        |mut state| async move {
            loop {
                if let Some(event) = state.pending.pop_front() {
                    return Some((Ok(event), state));
                }

                tokio::select! {
                    _ = state.poll.tick() => {
                        if let Ok(events) = poll_events(&state.store, &mut state.last_seen, &mut state.last_agents) {
                            state.pending.extend(events);
                        }
                    }
                    _ = state.heartbeat.tick() => {
                        return Some((Ok(heartbeat_event()), state));
                    }
                }
            }
        },
    );

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(KEEP_ALIVE_INTERVAL)
            .text("keep-alive"),
    )
}

fn poll_events(
    store: &Store,
    last_seen: &mut HashMap<String, TaskStatus>,
    last_agents: &mut HashMap<String, (String, Vec<String>)>,
) -> Result<VecDeque<Event>> {
    let mut events = Vec::new();
    let mut running_tasks = store.list_tasks(TaskFilter::Running)?;
    running_tasks.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let running_ids = running_tasks
        .iter()
        .map(|task| task.id.as_str().to_string())
        .collect::<HashSet<_>>();

    for task in &running_tasks {
        if status_changed(last_seen, task) {
            events.push(task_update_event(store, task)?);
        }
        last_seen.insert(task.id.as_str().to_string(), task.status);
    }

    let mut missing_ids = last_seen
        .keys()
        .filter(|id| !running_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    missing_ids.sort();
    for task_id in missing_ids {
        match store.get_task(&task_id)? {
            Some(task) => {
                if status_changed(last_seen, &task) {
                    events.push(task_update_event(store, &task)?);
                }
                if task.status.is_terminal() {
                    last_seen.remove(&task_id);
                } else {
                    last_seen.insert(task_id, task.status);
                }
            }
            None => {
                last_seen.remove(&task_id);
            }
        }
    }

    let today = store.list_tasks(TaskFilter::Today)?;
    events.push(fleet_summary_event(&today));
    append_agent_events(store, &running_tasks, last_agents, &mut events)?;
    Ok(events.into())
}

fn status_changed(last_seen: &HashMap<String, TaskStatus>, task: &Task) -> bool {
    last_seen
        .get(task.id.as_str())
        .map(|status| *status != task.status)
        .unwrap_or(true)
}

fn task_update_event(store: &Store, task: &Task) -> Result<Event> {
    let payload = TaskUpdatePayload {
        id: task.id.as_str().to_string(),
        status: task.status.as_str().to_string(),
        agent: task.agent_display_name().to_string(),
        tokens: task.tokens,
        cost_usd: task.cost_usd,
        duration_ms: task.duration_ms,
        milestone: store.latest_milestone(task.id.as_str())?,
        outcome: task.outcome().as_str().to_string(),
        verify_status: task.verify_status.as_str().to_string(),
        sector_id: task.project_id.clone().or_else(|| {
            task.repo_path.as_deref().and_then(|path| {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
        }),
        latest_error: store.latest_error(task.id.as_str()),
    };
    Ok(Event::default()
        .event(TASK_UPDATE_EVENT)
        .data(serialize_json(&payload)?))
}

fn append_agent_events(
    store: &Store,
    running: &[Task],
    last_agents: &mut HashMap<String, (String, Vec<String>)>,
    events: &mut Vec<Event>,
) -> Result<()> {
    let agents = fleet::build_agents(store, running)?;
    for agent in agents {
        let state = (agent.quota.state.clone(), agent.running_task_ids.clone());
        let changed = last_agents.get(&agent.name).is_some_and(|previous| previous != &state);
        if changed {
            events.push(agent_update_event(&agent)?);
        }
        last_agents.insert(agent.name, state);
    }
    Ok(())
}

fn agent_update_event(agent: &AgentResponse) -> Result<Event> {
    let payload = AgentUpdatePayload {
        name: agent.name.clone(),
        quota: agent.quota.clone(),
        busy: agent.busy,
        running_task_ids: agent.running_task_ids.clone(),
    };
    Ok(Event::default()
        .event(AGENT_UPDATE_EVENT)
        .data(serialize_json(&payload)?))
}

fn fleet_summary_event(tasks: &[Task]) -> Event {
    let payload: FleetSummary = fleet::summary_for_tasks(tasks, "today");
    let data = serialize_json(&payload).unwrap_or_else(|_| "{}".to_string());
    Event::default().event(FLEET_SUMMARY_EVENT).data(data)
}

fn heartbeat_event() -> Event {
    let payload = HeartbeatPayload {
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    let data = match serialize_json(&payload) {
        Ok(data) => data,
        Err(_) => "{\"timestamp\":\"\"}".to_string(),
    };
    Event::default().event(HEARTBEAT_EVENT).data(data)
}

fn serialize_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

#[cfg(test)]
mod tests {
    use super::TaskUpdatePayload;

    #[test]
    fn serializes_task_update_event_json() {
        let payload = TaskUpdatePayload {
            id: "t-1000".to_string(),
            status: "running".to_string(),
            agent: "codex".to_string(),
            tokens: Some(42),
            cost_usd: Some(0.12),
            duration_ms: Some(2500),
            milestone: Some("Investigating".to_string()),
            outcome: "running".to_string(),
            verify_status: "skipped".to_string(),
            sector_id: Some("repo".to_string()),
            latest_error: None,
        };

        let value = serde_json::to_value(&payload).expect("task update payload should serialize");
        let expected = serde_json::json!({
            "id": "t-1000",
            "status": "running",
            "agent": "codex",
            "tokens": 42,
            "cost_usd": 0.12,
            "duration_ms": 2500,
            "milestone": "Investigating",
            "outcome": "running",
            "verify_status": "skipped",
            "sector_id": "repo",
            "latest_error": null
        });

        assert_eq!(value, expected);
    }
}
