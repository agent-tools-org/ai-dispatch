// Match piped exec idle timing, descendant progress, and hung recovery evidence.
// Uses the shared activity sampler without allowing children to extend hard deadlines.

use super::*;
use crate::runtime::process::monitor;
use std::time::SystemTime;

impl Turn<'_> {
    pub(super) fn check_idle(&self) -> Result<()> {
        let run = self.sink.run;
        let idle = run.policy.idle;
        let activity = monitor::activity::latest(run.task.as_str(), Some(self.rpc.pid()), idle);
        if !expired(self.last_progress, activity, self.has_output, idle) {
            return Ok(());
        }
        monitor::insert_hung_detected_events(
            run.store,
            run.task,
            idle.as_secs(),
            self.sink.count,
            self.sink.last_detail.as_deref(),
            false,
        )?;
        bail!("Codex app-server idle timeout");
    }
}

fn expired(
    output: SystemTime,
    activity: Option<SystemTime>,
    saw_output: bool,
    idle: Duration,
) -> bool {
    let last = if saw_output {
        activity.unwrap_or(output).max(output)
    } else {
        output
    };
    last.elapsed().unwrap_or_default() >= idle
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;
