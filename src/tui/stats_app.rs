// App integration for refresh-time TUI statistics snapshots.
// Exports: private App methods for refresh and range changes.
// Deps: App store, tui::stats aggregation, and chrono/std time.

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Local};
use std::time::Duration;

use super::App;
use super::super::stats::aggregate_tasks;
const STATS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

impl App {
    pub(super) fn refresh_stats(&mut self) -> Result<()> {
        let now = Local::now();
        let end = now.date_naive();
        let tasks = self
            .store
            .list_stats_tasks(self.stats_range.query_start(end), end + ChronoDuration::days(1))?;
        self.stats = aggregate_tasks(&tasks, self.stats_range, now);
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
