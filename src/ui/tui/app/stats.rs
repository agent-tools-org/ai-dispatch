// App integration for refresh-time TUI statistics snapshots.
// Exports: private App methods for refresh and range changes.
// Deps: App store, tui::stats aggregation, and chrono/std time.

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Local};

use super::App;
use super::super::stats::aggregate_tasks;

impl App {
    pub(super) fn refresh_stats(&mut self) -> Result<()> {
        let now = Local::now();
        let end = now.date_naive();
        let tasks = self
            .store
            .list_stats_tasks(self.stats_range.query_start(end), end + ChronoDuration::days(1))?;
        self.stats = aggregate_tasks(&tasks, self.stats_range, now);
        Ok(())
    }

    pub(super) fn set_stats_range(&mut self, range: super::super::stats::StatsRange) -> Result<()> {
        if self.stats_range == range {
            return Ok(());
        }
        self.stats_range = range;
        self.refresh_requested = true;
        Ok(())
    }
}
