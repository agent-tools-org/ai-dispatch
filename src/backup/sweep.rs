// Reaper-tick backup sweep for tasks that ended outside the post-run lifecycle
// (reaper failures, stops, spawn and hook failures). Exports: sweep, sweep_at.
// Deps: Store backup candidate query, worker job specs, the shared attempt.

use chrono::{DateTime, Duration, Local};

use crate::store::Store;

/// Tasks that ended longer ago than this are never backed up, so enabling
/// `[backup]` on a project does not upload its history.
const WINDOW_SECS: i64 = 3600;
/// Quiet period after a task's last event or completion, so the path that ended
/// it has persisted its events and log before the bundle is taken.
const QUIET_SECS: i64 = 30;
const MAX_ATTEMPTS_PER_TICK: usize = 5;

/// Called once per reaper tick. Never fails the tick.
pub(crate) fn sweep(store: &Store) -> usize {
    sweep_at(store, Local::now(), worker_alive)
}

/// A task whose worker is still running has its post-run lifecycle in flight
/// (a long verify can outlast the quiet period); that lifecycle backs it up.
pub(super) fn worker_alive(task_id: &str) -> bool {
    crate::background::load_worker_pid(task_id)
        .ok()
        .flatten()
        .is_some_and(crate::background::is_process_running)
}

/// Attempts backup for unclaimed terminal tasks that ended within the window,
/// have been quiet for `QUIET_SECS` as of `now`, and have no live worker, at
/// most `MAX_ATTEMPTS_PER_TICK` attempts. Returns the number of attempts made.
pub(crate) fn sweep_at(store: &Store, now: DateTime<Local>, in_flight: impl Fn(&str) -> bool) -> usize {
    let oldest = now - Duration::seconds(WINDOW_SECS);
    let quiet_since = now - Duration::seconds(QUIET_SECS);
    let Ok(candidates) = store.backup_sweep_candidates(oldest, quiet_since) else {
        return 0;
    };
    let mut attempts = 0;
    for task_id in candidates {
        if attempts == MAX_ATTEMPTS_PER_TICK {
            break;
        }
        if !in_flight(&task_id) && super::attempt(store, &task_id) {
            attempts += 1;
        }
    }
    attempts
}
