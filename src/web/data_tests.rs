// Regression tests for web fields derived from real task runtime records.
// Exports: none.
// Deps: web handlers, background specs, Store, and task fixtures.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::Json;

use super::api::get_task;
use super::api_tests::make_task;
use super::fleet::{FleetParams, ServerInfo, get_fleet};
use crate::background::{BackgroundRunSpec, save_spec};
use crate::store::Store;
use crate::types::{AttributionSource, TaskStatus};

#[tokio::test(flavor = "current_thread")]
async fn running_task_reports_measured_worker_rss_and_fleet_sums_it() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-memory");
    task.status = TaskStatus::Pending;
    task.project_id = Some("memory-sector".to_string());
    store.insert_task(&task).unwrap();
    assert!(store.update_task_status("t-memory", TaskStatus::Running).unwrap());
    let worker_pid = std::process::id();
    let spec = serde_json::from_value::<BackgroundRunSpec>(serde_json::json!({
        "task_id": "t-memory",
        "worker_pid": worker_pid,
        "agent_name": "codex",
        "prompt": "measure memory",
        "retry": 0
    }))
    .unwrap();
    save_spec(&spec).unwrap();
    assert_eq!(crate::background::load_worker_pid("t-memory").unwrap(), Some(worker_pid));
    let expected_memory = crate::tui::metrics::get_process_metrics(worker_pid)
        .map(|metrics| metrics.memory_mb.round() as i64);

    let Json(response) = get_task(Path("t-memory".to_string()), State(store.clone())).await.unwrap();
    assert_eq!(response.memory_mb, expected_memory);

    let Json(fleet) = get_fleet(
        Query(FleetParams { window: Some("all".to_string()) }),
        State(store),
        axum::Extension(ServerInfo {
            host: "127.0.0.1".to_string(),
            port: 8080,
            started_at: "2026-08-20T07:00:00Z".to_string(),
            installed_agents: Vec::new(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(fleet.summary.memory_mb, expected_memory);
}

#[tokio::test(flavor = "current_thread")]
async fn sector_workgroup_is_derived_from_any_task_in_the_sector() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut first = make_task("t-sector-first");
    first.project_id = Some("shared-sector".to_string());
    first.workgroup_id = None;
    store.insert_task(&first).unwrap();
    let mut second = make_task("t-sector-second");
    second.project_id = Some("shared-sector".to_string());
    second.workgroup_id = Some("wg-real".to_string());
    store.insert_task(&second).unwrap();
    let mut ungrouped = make_task("t-sector-none");
    ungrouped.project_id = Some("ungrouped-sector".to_string());
    ungrouped.workgroup_id = None;
    store.insert_task(&ungrouped).unwrap();

    let Json(fleet) = get_fleet(
        Query(FleetParams { window: Some("all".to_string()) }),
        State(store),
        axum::Extension(ServerInfo {
            host: "127.0.0.1".to_string(),
            port: 8080,
            started_at: "2026-08-20T07:00:00Z".to_string(),
            installed_agents: Vec::new(),
        }),
    )
    .await
    .unwrap();
    let shared = fleet.sectors.iter().find(|sector| sector.id == "shared-sector").unwrap();
    let ungrouped = fleet.sectors.iter().find(|sector| sector.id == "ungrouped-sector").unwrap();
    assert_eq!(shared.workgroup_id.as_deref(), Some("wg-real"));
    assert_eq!(ungrouped.workgroup_id, None);
}

#[tokio::test(flavor = "current_thread")]
async fn fleet_reports_the_latest_observed_model_for_an_agent() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-observed-model");
    task.observed_model = Some("gpt-observed".to_string());
    task.attribution_source = Some(AttributionSource::Echoed);
    store.insert_task(&task).unwrap();

    let Json(fleet) = get_fleet(
        Query(FleetParams { window: Some("all".to_string()) }),
        State(store),
        axum::Extension(ServerInfo {
            host: "127.0.0.1".to_string(),
            port: 8080,
            started_at: "2026-08-20T07:00:00Z".to_string(),
            installed_agents: Vec::new(),
        }),
    )
    .await
    .unwrap();
    let codex = fleet.agents.iter().find(|agent| agent.name == "codex").unwrap();
    assert_eq!(codex.observed_model.as_deref(), Some("gpt-observed"));
}
