// Rate-limit detection: writes markers and keeps public hold facades.
// Classification and snapshot override live in route_availability.
// Exports: mark_rate_limited{,_for_message}, is_rate_limited,
// dispatch_blocking_hold, get_rate_limit_info, active_group_holds,
// format_hold_end, clear_*.

use crate::support::paths::aid_dir;
use crate::routing::availability::{
    Hold, HoldEnd, MANUAL_HOLD, RouteAvailability, RouteStatus, StoredHold, availability,
    availability_for_group, availability_for_model, classify_hold, format_hold_end_for,
    stored_hold,
};
use crate::types::AgentKind;
use chrono::{DateTime, Local, NaiveDateTime};
#[cfg(test)]
use chrono::{Datelike, Timelike};
use std::fs;
use std::path::PathBuf;

mod claude;
pub(crate) use claude::{claude_rejected_limit, parse_claude_reset_clock, parse_recovery_time};

mod identity;
pub(crate) use identity::stored_signature_agent;

mod markers;
pub use markers::*;
mod state;
use state::discovered_group_markers;
pub use state::*;
mod detection;

#[cfg(test)]
pub(crate) use detection::test_future_recovery_time;
pub use detection::*;
pub(crate) use detection::{parse_recovery_datetime, refusal_on_channel};
mod info;
pub use info::*;
pub(crate) use markers::{format_recovery, group_marker_path, marker_field, marker_path};

/// Cooldown for a refusal that named no reset time and matched no signature —
/// a bare 429 or 402 seen on stderr. All we know is that it just happened, so
/// the route is stepped over briefly and then tried again. Anything longer
/// would be an invented outage for a route that is probably still serving.
pub(crate) const RATE_LIMIT_WINDOW_SECS: u64 = 300;

#[cfg(test)]
fn assert_marker_path_isolated() {
    crate::support::paths::assert_aid_home_isolated("rate_limit::marker_path");
}

/// Resolve a CLI agent name into kind + optional custom id for marker paths.
pub fn resolve_agent(name: &str) -> (AgentKind, Option<&str>) {
    match AgentKind::parse_str(name) {
        Some(kind) => (kind, None),
        None => (AgentKind::Custom, Some(name)),
    }
}

/// How a live hold ends, phrased for the caller who just chose this agent —
/// `None` when nothing should stop the dispatch.
///
/// `aid run` gated on the presence of a recovery time, which was wrong in both
/// directions: a marker whose stated time had already passed still diverted the
/// run, and a refusal only a person can end carries no time at all, so dispatch
/// walked straight into an account that cannot serve.
///
/// The bounded transient cooldown is deliberately not a gate. It is short enough
/// that moving the caller off the agent they asked for costs more than the wait,
/// and gating on it was never the previous behaviour either.
pub fn dispatch_blocking_hold(agent: &AgentKind, custom_name: Option<&str>) -> Option<String> {
    hold_text(agent, custom_name, &availability(agent, custom_name))
}

pub fn dispatch_blocking_hold_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> Option<String> {
    hold_text(
        agent,
        custom_name,
        &availability_for_model(agent, custom_name, model),
    )
}

fn hold_text(
    agent: &AgentKind,
    custom_name: Option<&str>,
    avail: &RouteAvailability,
) -> Option<String> {
    match avail.status {
        RouteStatus::Held => Some(dispatch_hold_end(agent, custom_name, avail)),
        _ => None,
    }
}

fn dispatch_hold_end(
    agent: &AgentKind,
    custom_name: Option<&str>,
    avail: &RouteAvailability,
) -> String {
    match &avail.ends {
        HoldEnd::At(_) => avail
            .why
            .strip_prefix("held ")
            .unwrap_or(&avail.why)
            .to_string(),
        HoldEnd::ClearLimit { slug } => {
            format!("until cleared with `aid config clear-limit {slug}`")
        }
        HoldEnd::SnapshotDatedWindow => {
            format_hold_end_for(&StoredHold::Windowed, agent, custom_name, None)
        }
        HoldEnd::Cooldown | HoldEnd::Nothing => avail.why.clone(),
    }
}

#[cfg(test)]
#[path = "credibility_tests.rs"]
mod credibility_tests;

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod recovery_tests;
#[cfg(test)]
#[path = "rate_limit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "stale_tests.rs"]
mod stale_clear_tests;

#[cfg(test)]
#[path = "hold_tests.rs"]
mod hold_tests;

#[cfg(test)]
#[path = "custom_tests.rs"]
mod custom_tests;

#[cfg(test)]
mod home_guard_tests {
    use super::*;
    use crate::support::paths::{self};

    #[test]
    fn marker_path_writes_under_isolated_home() {
        let temp = crate::support::test_env::TestHome::new();
        std::fs::create_dir_all(paths::aid_dir()).unwrap();

        mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");
        let marker = paths::aid_dir().join("rate-limit-codex");
        assert!(marker.exists());
        assert!(marker.starts_with(temp.path()));
    }

    #[test]
    fn marker_path_refuses_real_home_without_guard() {
        let resolved = paths::aid_dir();
        let home = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let real = home.join(".aid");
        if resolved != real {
            return;
        }
        let err = std::panic::catch_unwind(|| {
            let _ = marker_path(&AgentKind::Codex, None);
        });
        assert!(
            err.is_err(),
            "marker_path must refuse real ~/.aid without AidHomeGuard"
        );
    }
}

pub(crate) mod signatures;
pub(crate) mod wait;
