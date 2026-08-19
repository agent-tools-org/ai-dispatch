// Foreground signal classification and non-interactive detach handling.
// Exports signal waiting, tty-aware detach decisions, and the hard-exit path.
// Deps: stdin terminal detection, task events, background spec, and the store.

use anyhow::Result;
use chrono::Local;
use std::io::{IsTerminal, Write};

use crate::background;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ForegroundSignal {
    Int,
    Term,
    Hup,
}

impl ForegroundSignal {
    pub(super) fn name(self) -> &'static str {
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

/// Non-interactive SIGTERM/SIGHUP detaches: the harness timed out, the operator
/// did not press Ctrl-C. SIGINT always stops (it means interrupt, not timeout).
pub(super) fn should_detach(signal: ForegroundSignal) -> bool {
    should_detach_for_terminal(signal, std::io::stdin().is_terminal())
}

pub(super) fn should_detach_for_terminal(
    signal: ForegroundSignal,
    stdin_is_terminal: bool,
) -> bool {
    matches!(signal, ForegroundSignal::Term | ForegroundSignal::Hup) && !stdin_is_terminal
}

/// Mark the spec as deliberately detached, record a Milestone, print the
/// reattach hint, and hard-exit. The hard exit skips Drop impls (tokio Child,
/// PTY master, ForegroundSpecGuard) that would kill the agent. The `detached`
/// marker tells the reaper to adopt the task on the next `aid` invocation
/// instead of reaping the dead worker_pid and killing agent_pid.
pub(super) fn handle_foreground_detach(
    store: &Store,
    task_id: &TaskId,
    signal: ForegroundSignal,
) -> ! {
    let detail = format!(
        "foreground aid detached after {} without signaling the agent",
        signal.name()
    );
    if let Some(mut spec) = background::load_spec_if_exists(task_id.as_str())
        .ok()
        .flatten()
    {
        spec.detached = true;
        let _ = background::save_spec(&spec);
    }
    let insert_result = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: None,
    });
    let mut stderr = std::io::stderr().lock();
    if let Err(err) = insert_result {
        let _ = writeln!(stderr, "[aid] Failed to record detach milestone for {task_id}: {err}");
    }
    let _ = writeln!(
        stderr,
        "\n\u{26a0} Received {} (non-interactive) \u{2014} aid detached from task {task_id}.\n\
         Aid did not send a stop signal. Reattach: aid watch --wait {task_id}",
        signal.name()
    );
    std::process::exit(signal.exit_code());
}

// SIGKILL cannot be caught; the foreground spec remains the convergence backstop for that case.
#[cfg(unix)]
pub(super) async fn wait_for_foreground_signal() -> Result<ForegroundSignal> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;
    tokio::select! {
        _ = sigint.recv() => Ok(ForegroundSignal::Int),
        _ = sigterm.recv() => Ok(ForegroundSignal::Term),
        _ = sighup.recv() => Ok(ForegroundSignal::Hup),
    }
}

#[cfg(not(unix))]
pub(super) async fn wait_for_foreground_signal() -> Result<ForegroundSignal> {
    std::future::pending::<Result<ForegroundSignal>>().await
}
