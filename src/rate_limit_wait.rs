// Background wait for a declared reset. Clock and dated windows only.
// Exports: wait_for_declared_reset. Prepaid / plan-change / unmapped refuse.
// Deps: route_availability, rate_limit, store.

use anyhow::Result;
use chrono::{Local, NaiveDateTime, Utc};

use crate::live_quota;
use crate::route_availability::{HoldEnd, ProbeEvidence, QuotaWall, RouteStatus};
use crate::store::Store;
use crate::types::{AgentKind, TaskUrgency};

const POLL_INTERVAL_SECS: u64 = 5;

#[derive(Debug, PartialEq)]
pub(crate) enum WaitDecision {
    Ready,
    Until(NaiveDateTime),
    Refuse { message: String },
}

fn wall_label(wall: QuotaWall) -> &'static str {
    match wall {
        QuotaWall::Clock => "clock",
        QuotaWall::Windowed => "windowed",
        QuotaWall::Prepaid => "prepaid",
        QuotaWall::PlanChange => "plan-change",
        QuotaWall::Transient => "transient",
        QuotaWall::None => "none",
    }
}

fn earliest_future_reset(probe: Option<&ProbeEvidence>) -> Option<NaiveDateTime> {
    let now = Utc::now();
    probe?
        .windows
        .iter()
        .filter_map(|window| window.resets_at)
        .filter(|at| *at > now)
        .min()
        .map(|at| at.with_timezone(&Local).naive_local())
}

pub(crate) fn wait_decision(agent: &AgentKind, custom_name: Option<&str>) -> WaitDecision {
    let avail = crate::route_availability::availability(agent, custom_name);
    if avail.status != RouteStatus::Held {
        return WaitDecision::Ready;
    }
    let slug = crate::rate_limit::marker_slug(agent, custom_name);
    match avail.wall {
        QuotaWall::Clock => match avail.ends {
            HoldEnd::At(at) if at > Local::now().naive_local() => WaitDecision::Until(at),
            _ => WaitDecision::Ready,
        },
        QuotaWall::Windowed if live_quota::provider_name(agent).is_some() => {
            match earliest_future_reset(avail.probe.as_ref()) {
                Some(at) => WaitDecision::Until(at),
                None => WaitDecision::Refuse {
                    message: format!(
                        "{slug} is windowed but has no future snapshot date; \
                         background wait will not poll. Use `aid config clear-limit {slug}` \
                         or pick another agent."
                    ),
                },
            }
        }
        QuotaWall::Prepaid | QuotaWall::PlanChange | QuotaWall::Windowed => {
            WaitDecision::Refuse {
                message: format!(
                    "{slug} is held ({}); background wait will not poll. \
                     Use `aid config clear-limit {slug}` or pick another agent.",
                    wall_label(avail.wall)
                ),
            }
        }
        QuotaWall::Transient | QuotaWall::None => WaitDecision::Ready,
    }
}

pub(crate) async fn wait_for_declared_reset(
    store: &Store,
    task_id: &str,
    agent: AgentKind,
    custom_name: Option<&str>,
) -> Result<()> {
    let profile = store.get_task_profile(task_id)?;
    if profile.urgency != Some(TaskUrgency::Background) {
        return Ok(());
    }
    match wait_decision(&agent, custom_name) {
        WaitDecision::Ready => Ok(()),
        WaitDecision::Refuse { message } => {
            aid_warn!("[aid] {message}");
            Ok(())
        }
        WaitDecision::Until(deadline) => {
            while crate::rate_limit::is_rate_limited(&agent, custom_name) {
                if Local::now().naive_local() >= deadline {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "rate_limit_wait_tests.rs"]
mod tests;
