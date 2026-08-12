// App integration for refresh-time TUI statistics snapshots.
// Exports: private App methods for refresh and range changes.
// Deps: App store, tui::stats aggregation, and chrono/std time.

use anyhow::Result;
use chrono::Local;
use std::time::Duration;

use super::App;
use super::super::stats::aggregate_tasks;
use crate::types::TaskFilter;

const STATS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

impl App {
    pub(super) fn refresh_stats(&mut self) -> Result<()> {
        let tasks = self.store.list_tasks(TaskFilter::All)?;
        self.stats = aggregate_tasks(&tasks, self.stats_range, Local::now());
        self.last_stats_refresh = std::time::Instant::now();
        Ok(())
    }

    pub(super) fn refresh_stats_if_due(&mut self) -> Result<()> {
        if self.stats_mode && self.last_stats_refresh.elapsed() >= STATS_REFRESH_INTERVAL {
            self.refresh_stats()?;
        }
        Ok(())
    }

    pub(super) fn set_stats_range(&mut self, range: super::super::stats::StatsRange) -> Result<()> {
        if self.stats_range == range {
            return Ok(());
        }
        self.stats_range = range;
        self.refresh_stats()
    }
}
