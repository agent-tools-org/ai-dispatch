// Foreground attachment to a background worker.
// Exports wait_for_task; depends on Store, background specs, and task outcomes.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::background;
use crate::store::Store;
use crate::types::{verify_required, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};

#[cfg(unix)]
use tokio::signal::unix::{signal, Signal, SignalKind};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) async fn wait_for_task(store: &Arc<Store>, task_id: &TaskId) -> Result<TaskId> {
    let mut signals = SignalWaiter::new().await?;
    let mut current_id = task_id.clone();
    let mut reported_status = None;
    let mut reported_events = 0;
    let mut reported_terminal = false;
    loop {
        let _ = background::check_zombie_tasks(store);
        let task = store
            .get_task(current_id.as_str())?
            .ok_or_else(|| anyhow::anyhow!("Task '{}' disappeared while running", current_id))?;
        report_events(store, current_id.as_str(), &mut reported_events)?;
        report_progress(&task, &mut reported_status);
        if task_is_complete(&task) {
            if !reported_terminal {
                print_completion(store, &task);
                reported_terminal = true;
            }
            if let Some(retry) = latest_retry(store, current_id.as_str())? {
                current_id = retry.id;
                reported_status = None;
                reported_events = 0;
                reported_terminal = false;
                continue;
            }
            if background::load_spec_if_exists(current_id.as_str())?.is_none() {
                return Ok(current_id);
            }
        }
        tokio::select! {
            signal = signals.next() => handle_signal(store, &current_id, signal?),
            _ = sleep(POLL_INTERVAL) => {}
        }
    }
}

fn report_events(store: &Store, task_id: &str, reported_events: &mut usize) -> Result<()> {
    let events = store.get_events(task_id)?;
    for event in events.iter().skip(*reported_events) {
        if should_report_event(event) {
            aid_info!("[aid] [{}] {}", event.event_kind.as_str(), event.detail);
        }
    }
    *reported_events = events.len();
    Ok(())
}

fn should_report_event(event: &TaskEvent) -> bool {
    event.event_kind.is_progress()
        && !matches!(event.event_kind, EventKind::Completion | EventKind::NoOp)
        && !event.detail.trim().is_empty()
}

fn report_progress(task: &Task, reported_status: &mut Option<TaskStatus>) {
    if *reported_status == Some(task.status) {
        return;
    }
    *reported_status = Some(task.status);
    if !task.status.is_terminal() && task.status != TaskStatus::Running {
        aid_info!("[aid] {} {}", task.id, task.status.label());
    }
}

fn task_is_complete(task: &Task) -> bool {
    if !task.status.is_terminal() {
        return false;
    }
    !(matches!(task.verify_status, VerifyStatus::Pending)
        && verify_required(task.verify.as_deref()))
}

fn latest_retry(store: &Store, parent_id: &str) -> Result<Option<Task>> {
    Ok(store.get_direct_retries(parent_id)?.into_iter().last())
}

fn print_completion(store: &Store, task: &Task) {
    let duration = format_duration(task.duration_ms.unwrap_or_default());
    let tokens = task
        .tokens
        .map(|value| format!(", {value} tokens"))
        .unwrap_or_default();
    let cost = task
        .cost_usd
        .map(|value| format!(", {}", crate::cost::format_cost(Some(value))))
        .unwrap_or_default();
    let reason = if task.status == TaskStatus::Failed {
        store
            .latest_error(task.id.as_str())
            .map(|value| format!("\n[aid] Reason: {value}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    println!(
        "Task {} {} ({}{}{}){}",
        task.id,
        task.status.label(),
        duration,
        tokens,
        cost,
        reason
    );
}

fn format_duration(milliseconds: i64) -> String {
    let seconds = milliseconds.max(0) / 1_000;
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

fn handle_signal(store: &Arc<Store>, task_id: &TaskId, signal: ForegroundSignal) -> ! {
    match signal {
        ForegroundSignal::Int => {
            if let Err(error) = crate::cmd::stop::terminate_any(store, task_id.as_str()) {
                aid_error!("[aid] Failed to stop task {task_id}: {error}");
            }
            std::process::exit(130);
        }
        ForegroundSignal::Term | ForegroundSignal::Hup => {
            let name = signal.name();
            aid_hint!(
                "\n⚠ Received {name} — aid detached from task {task_id}.\n\
                 Aid did not send a stop signal. Reattach: aid watch --wait {task_id}"
            );
            std::process::exit(signal.exit_code());
        }
    }
}

#[derive(Clone, Copy)]
enum ForegroundSignal {
    Int,
    Term,
    Hup,
}

impl ForegroundSignal {
    fn name(self) -> &'static str {
        match self {
            Self::Int => "SIGINT",
            Self::Term => "SIGTERM",
            Self::Hup => "SIGHUP",
        }
    }

    fn exit_code(self) -> i32 {
        match self {
            Self::Int => 130,
            Self::Term => 143,
            Self::Hup => 129,
        }
    }
}

struct SignalWaiter {
    #[cfg(unix)]
    sigint: Signal,
    #[cfg(unix)]
    sigterm: Signal,
    #[cfg(unix)]
    sighup: Signal,
}

impl SignalWaiter {
    async fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            return Ok(Self {
                sigint: signal(SignalKind::interrupt())?,
                sigterm: signal(SignalKind::terminate())?,
                sighup: signal(SignalKind::hangup())?,
            });
        }
        #[cfg(not(unix))]
        Ok(Self {})
    }

    async fn next(&mut self) -> Result<ForegroundSignal> {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.sigint.recv() => Ok(ForegroundSignal::Int),
                _ = self.sigterm.recv() => Ok(ForegroundSignal::Term),
                _ = self.sighup.recv() => Ok(ForegroundSignal::Hup),
            }
        }
        #[cfg(not(unix))]
        {
            std::future::pending::<Result<ForegroundSignal>>().await
        }
    }
}

#[cfg(test)]
#[path = "run_foreground_watch_tests.rs"]
mod tests;
