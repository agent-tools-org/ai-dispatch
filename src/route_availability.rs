// Single source of truth for whether a route can take work.
// Exports: RouteAvailability and availability / availability_for_{model,group}.
// Deps: live_quota snapshots, rate_limit markers, route_availability_policy.

use chrono::{DateTime, Local, NaiveDateTime, Utc};
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::live_quota;
use crate::rate_limit::{group_marker_path, marker_field, marker_path, marker_slug};
use crate::types::AgentKind;

#[path = "route_availability_policy.rs"]
mod policy;

pub(crate) use policy::{
    classify_hold, format_hold_end_for, marker_text_from_info, overrides_marker_at,
    snapshot_overrides, stored_hold, wall_of, Hold, StoredHold, MANUAL_HOLD,
};

#[cfg(test)]
pub(crate) use policy::overrides_marker_at_in_cache;

const DEGRADED_USED: f64 = 80.0;

/// What is actually stopping work on this route, if anything.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteAvailability {
    pub status: RouteStatus,
    pub wall: QuotaWall,
    pub ends: HoldEnd,
    pub why: String,
    pub marker: Option<MarkerEvidence>,
    pub probe: Option<ProbeEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteStatus {
    Dispatchable,
    Degraded,
    Held,
}

/// The thing that has to change for this route to serve again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaWall {
    Clock,
    Windowed,
    Prepaid,
    PlanChange,
    Transient,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HoldEnd {
    At(NaiveDateTime),
    ClearLimit { slug: String },
    SnapshotDatedWindow,
    Cooldown,
    Nothing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkerEvidence {
    pub mtime: SystemTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProbeEvidence {
    pub provider: String,
    pub fetched_at: DateTime<Utc>,
    pub age: Duration,
    pub stale: bool,
    pub ok: bool,
    pub windows: Vec<WindowView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowView {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub group: Option<String>,
}

pub fn availability(agent: &AgentKind, custom_name: Option<&str>) -> RouteAvailability {
    from_path(agent, custom_name, None, &marker_path(agent, custom_name))
}

pub fn availability_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> RouteAvailability {
    match crate::agent::model_group::model_group(*agent, model) {
        Some(group) => availability_for_group(agent, custom_name, group),
        None => availability(agent, custom_name),
    }
}

pub fn availability_for_group(
    agent: &AgentKind,
    custom_name: Option<&str>,
    group: &str,
) -> RouteAvailability {
    from_path(
        agent,
        custom_name,
        Some(group),
        &group_marker_path(agent, custom_name, group),
    )
}

fn from_path(
    agent: &AgentKind,
    custom_name: Option<&str>,
    group: Option<&str>,
    path: &Path,
) -> RouteAvailability {
    let content = std::fs::read_to_string(path).ok();
    let mtime = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    decide(
        agent,
        custom_name,
        group,
        content.as_deref(),
        mtime,
        live_quota::snapshot(agent),
    )
}

pub(crate) fn decide(
    agent: &AgentKind,
    custom_name: Option<&str>,
    group: Option<&str>,
    content: Option<&str>,
    marker_mtime: Option<SystemTime>,
    snapshot: Option<ProbeEvidence>,
) -> RouteAvailability {
    let hold = content.map(|text| stored_hold(text, agent));
    let wall = content
        .map(|text| wall_of(agent, text))
        .unwrap_or(QuotaWall::None);
    if let (Some(hold), Some(mtime), Some(probe)) = (hold.as_ref(), marker_mtime, snapshot.as_ref())
    {
        let relevant = policy::relevant_windows(probe, agent, group);
        if snapshot_overrides(hold, probe, mtime, &relevant) {
            return released(wall, snapshot, marker_mtime);
        }
    }
    match hold {
        Some(stored) => from_stored(
            agent,
            custom_name,
            stored,
            wall,
            marker_mtime,
            snapshot,
            content,
        ),
        None => probe_only(snapshot),
    }
}

fn released(
    wall: QuotaWall,
    snapshot: Option<ProbeEvidence>,
    marker_mtime: Option<SystemTime>,
) -> RouteAvailability {
    let used = max_used(snapshot.as_ref());
    let stale = snapshot.as_ref().is_some_and(|probe| probe.stale);
    let status = if !stale && used >= DEGRADED_USED {
        RouteStatus::Degraded
    } else {
        RouteStatus::Dispatchable
    };
    let why = match snapshot.as_ref() {
        Some(probe) => format!("aidbar {} {:.1}% newer than marker", probe.provider, used),
        None => "snapshot released hold".to_string(),
    };
    RouteAvailability {
        status,
        wall,
        ends: HoldEnd::Nothing,
        why,
        marker: marker_mtime.map(|mtime| MarkerEvidence { mtime }),
        probe: snapshot,
    }
}

fn from_stored(
    agent: &AgentKind,
    custom_name: Option<&str>,
    hold: StoredHold,
    wall: QuotaWall,
    marker_mtime: Option<SystemTime>,
    snapshot: Option<ProbeEvidence>,
    content: Option<&str>,
) -> RouteAvailability {
    let slug = marker_slug(agent, custom_name).to_string();
    let marker = marker_mtime.map(|mtime| MarkerEvidence { mtime });
    match hold {
        StoredHold::Until(at) if at > Local::now().naive_local() => RouteAvailability {
            status: RouteStatus::Held,
            wall,
            ends: HoldEnd::At(at),
            why: stated_clock_why(content, at),
            marker,
            probe: snapshot,
        },
        StoredHold::Windowed => RouteAvailability {
            status: RouteStatus::Held,
            wall,
            ends: HoldEnd::SnapshotDatedWindow,
            why: "held until a dated snapshot with headroom".to_string(),
            marker,
            probe: snapshot,
        },
        StoredHold::NeedsHuman => RouteAvailability {
            status: RouteStatus::Held,
            wall,
            ends: HoldEnd::ClearLimit { slug: slug.clone() },
            why: format!("held until `aid config clear-limit {slug}`"),
            marker,
            probe: snapshot,
        },
        StoredHold::Transient if cooldown_active(marker_mtime) => RouteAvailability {
            status: RouteStatus::Degraded,
            wall,
            ends: HoldEnd::Cooldown,
            why: "transient cooldown".to_string(),
            marker,
            probe: snapshot,
        },
        StoredHold::Until(_) | StoredHold::Transient => probe_only(snapshot),
    }
}

fn probe_only(snapshot: Option<ProbeEvidence>) -> RouteAvailability {
    let used = max_used(snapshot.as_ref());
    let degraded = snapshot
        .as_ref()
        .is_some_and(|probe| probe.ok && !probe.stale && used >= DEGRADED_USED);
    RouteAvailability {
        status: if degraded {
            RouteStatus::Degraded
        } else {
            RouteStatus::Dispatchable
        },
        wall: QuotaWall::None,
        ends: HoldEnd::Nothing,
        why: if degraded {
            format!("live window {used:.1}%")
        } else {
            "no hold".to_string()
        },
        marker: None,
        probe: snapshot,
    }
}

fn max_used(snapshot: Option<&ProbeEvidence>) -> f64 {
    snapshot
        .map(|probe| {
            probe
                .windows
                .iter()
                .map(|window| window.used_percent)
                .fold(0.0_f64, f64::max)
        })
        .unwrap_or(0.0)
}

fn stated_clock_why(content: Option<&str>, at: NaiveDateTime) -> String {
    let stated = content
        .and_then(|text| marker_field(text, "recovery_at: "))
        .unwrap_or_else(|| at.format("%b %d, %Y %I:%M %p").to_string());
    format!("held until {stated}")
}

fn cooldown_active(mtime: Option<SystemTime>) -> bool {
    mtime
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed.as_secs() < crate::rate_limit::RATE_LIMIT_WINDOW_SECS)
}

#[cfg(test)]
#[path = "route_availability_tests.rs"]
mod tests;
