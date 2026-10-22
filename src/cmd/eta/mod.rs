// ETA estimation helpers for running tasks in board views.
// Exports: estimate_eta for display-ready remaining-time labels.
// Deps: chrono, crate::store::Store, crate::types::Task.

use chrono::{DateTime, Local};

use crate::store::Store;
use crate::types::{Task, TaskStatus};

pub fn estimate_eta(task: &Task, store: &Store) -> Option<String> {
    estimate_eta_at(task, store, Local::now())
}

pub fn estimate_progress(task: &Task, store: &Store) -> Option<u8> {
    estimate_progress_at(task, store, Local::now())
}

fn estimate_eta_at(task: &Task, store: &Store, now: DateTime<Local>) -> Option<String> {
    if task.status != TaskStatus::Running {
        return None;
    }
    let elapsed_ms = (now - task.created_at).num_milliseconds();
    let mut durations: Vec<i64> = store
        .recent_tasks_for_agent(task.agent, 50)
        .ok()?
        .into_iter()
        .filter_map(|entry| entry.duration_ms)
        .collect();
    if durations.len() < 3 {
        return None;
    }
    durations.sort_unstable();
    let median_ms = durations[durations.len() / 2];
    let remaining_ms = median_ms - elapsed_ms;
    if remaining_ms <= 0 {
        return Some("any moment".to_string());
    }
    Some(format_eta(remaining_ms))
}

fn estimate_progress_at(task: &Task, store: &Store, now: DateTime<Local>) -> Option<u8> {
    if task.status != TaskStatus::Running {
        return None;
    }
    let elapsed_ms = (now - task.created_at).num_milliseconds();
    let durations: Vec<i64> = store
        .recent_tasks_for_agent(task.agent, 50)
        .ok()?
        .into_iter()
        .filter_map(|entry| entry.duration_ms)
        .collect();
    if durations.len() < 3 {
        return None;
    }
    let mut sorted = durations;
    sorted.sort_unstable();
    let median_ms = sorted[sorted.len() / 2];
    if median_ms <= 0 {
        return None;
    }
    let pct = ((elapsed_ms as f64 / median_ms as f64) * 100.0)
        .min(99.0)
        .max(0.0) as u8;
    Some(pct)
}

fn format_eta(ms: i64) -> String {
    let secs = (ms / 1_000).max(0);
    if secs < 60 {
        format!("~{secs}s")
    } else if secs < 3_600 {
        format!("~{}m", secs / 60)
    } else {
        format!("~{}h{}m", secs / 3_600, (secs % 3_600) / 60)
    }
}

#[cfg(test)]
#[path = "eta_tests.rs"]
mod tests;
