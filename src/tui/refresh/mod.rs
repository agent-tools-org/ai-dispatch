// Single-flight background snapshots for the interactive dashboard.
// Exports RefreshWorker; keeps database and process I/O off the terminal thread.
// Deps: App, Store, bounded std channels and a dedicated worker thread.

use super::App;
use anyhow::{Result, anyhow};
use std::sync::{Arc, mpsc};

#[derive(Clone, PartialEq, Eq)]
struct Request {
    show_all: bool,
    detail_task: Option<String>,
    dashboard: bool,
    multipane: bool,
    stats: Option<crate::tui::stats::StatsRange>,
}

impl Request {
    fn capture(app: &App) -> Self {
        Self {
            show_all: app.show_all,
            detail_task: app.detail_mode.then(|| app.selected_task())
                .flatten().map(|task| task.id.to_string()),
            dashboard: app.dashboard_mode,
            multipane: app.multipane_mode,
            stats: app.stats_mode.then_some(app.stats_range),
        }
    }
}

pub(in crate::tui) struct RefreshWorker {
    requests: mpsc::SyncSender<Request>,
    results: mpsc::Receiver<(Request, Result<App, String>)>,
    in_flight: bool,
}

impl RefreshWorker {
    pub(in crate::tui) fn start(app: &App) -> Result<Self> {
        let (requests, incoming) = mpsc::sync_channel::<Request>(1);
        let (outgoing, results) = mpsc::sync_channel(1);
        let store = Arc::clone(&app.store);
        let task_id = app.task_id_filter.clone();
        let group = app.group_filter.clone();
        std::thread::Builder::new().name("aid-tui-refresh".into()).spawn(move || {
            while let Ok(request) = incoming.recv() {
                let options = crate::tui::RunOptions { task_id: task_id.clone(), group: group.clone() };
                let result = load_snapshot(store.clone(), options, &request)
                    .map_err(|error| format!("{error:#}"));
                if outgoing.send((request, result)).is_err() { break; }
            }
        })?;
        Ok(Self { requests, results, in_flight: false })
    }

    pub(in crate::tui) fn poll(&mut self, app: &mut App) -> Result<()> {
        match self.results.try_recv() {
            Ok((request, result)) => {
                self.in_flight = false;
                if request == Request::capture(app) {
                    match result {
                        Ok(snapshot) => app.apply_snapshot(snapshot),
                        Err(error) => app.refresh_status = Some(format!("Refresh failed: {error}")),
                    }
                } else {
                    app.refresh_requested = true;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => return Err(anyhow!("TUI refresh worker stopped")),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if app.refresh_requested && !self.in_flight {
            self.requests.send(Request::capture(app))?;
            self.in_flight = true;
            app.refresh_requested = false;
            app.refresh_status = Some("Refreshing…".into());
        }
        Ok(())
    }
}

fn load_snapshot(
    store: Arc<crate::store::Store>,
    options: crate::tui::RunOptions,
    request: &Request,
) -> Result<App> {
    let mut app = App::empty(store, options);
    app.show_all = request.show_all;
    app.reload_tasks()?;
    app.metrics = app.load_metrics(&app.tasks);
    if let Some(task_id) = &request.detail_task {
        if let Some(index) = app.tasks.iter().position(|task| task.id.as_str() == task_id) {
            app.selected = index;
            app.load_selected_events()?;
            let output = crate::task_view::read_output(&app.tasks[index]);
            app.output_cache.insert(task_id.clone(), output);
        }
    }
    if request.dashboard { app.load_dashboard_events()?; }
    if request.multipane { app.load_multipane_events()?; }
    if let Some(range) = request.stats {
        app.stats_range = range;
        app.refresh_stats()?;
    }
    Ok(app)
}

impl App {
    fn apply_snapshot(&mut self, snapshot: App) {
        let selected_id = self.selected_task().map(|task| task.id.clone());
        let anchor = self.nodes.get(self.tree_selected).map(|node| {
            (node.task_id.to_string(), node.project_id.clone(), node.is_group_header)
        });
        self.tasks = snapshot.tasks;
        self.latest_events = snapshot.latest_events;
        self.milestones = snapshot.milestones;
        self.wg_creators = snapshot.wg_creators;
        self.metrics = snapshot.metrics;
        self.events_cache = snapshot.events_cache;
        self.output_cache = snapshot.output_cache;
        if self.stats_mode { self.stats = snapshot.stats; }
        self.selected = selected_id.and_then(|id| self.tasks.iter().position(|task| task.id == id))
            .unwrap_or_else(|| self.selected.min(self.tasks.len().saturating_sub(1)));
        if self.collapsed_projects.is_empty() {
            self.nodes = snapshot.nodes;
            self.tree_node_count = self.nodes.len();
        } else {
            self.rebuild_nodes();
        }
        self.tree_selected = Self::resolve_tree_selected(&self.nodes, anchor, self.tree_selected);
        self.reconcile_active_pane();
        self.clamp_all_pane_scrolls();
        self.clamp_detail_scroll();
        self.refresh_status = None;
    }
}

#[cfg(test)]
mod tests;
