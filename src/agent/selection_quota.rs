// Live-quota ranking terms and advise candidate quota picture.
// Headroom penalizes a filling window; unused quota never boosts.
// Deps: route_availability (SoT), rate_limit group holds, declared urgency.

use serde::{Deserialize, Serialize};

use crate::rate_limit;
use crate::route_availability::{
    availability, ProbeEvidence, QuotaWall, RouteAvailability, RouteStatus, WindowView,
};
use crate::types::{AgentKind, TaskUrgency};

pub(super) fn headroom_penalty(kind: AgentKind) -> f64 {
    let avail = availability(&kind, None);
    if avail.status == RouteStatus::Held {
        return 0.0;
    }
    let Some(probe) = avail.probe.as_ref() else {
        return 0.0;
    };
    if !probe.ok || probe.stale {
        return 0.0;
    }
    match tightest_window(&probe.windows) {
        Some(window) => penalty_from_used(window.used_percent),
        None => 0.0,
    }
}

pub(super) fn penalty_from_used(used: f64) -> f64 {
    if used < 50.0 {
        0.0
    } else if used < 80.0 {
        -1.0
    } else if used < 95.0 {
        -3.0
    } else {
        -6.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CandidateQuota {
    pub status: String,
    pub wall: String,
    pub used_percent: Option<f64>,
    pub resets_at: Option<String>,
    pub freshness_secs: Option<u64>,
    pub stale: bool,
    pub source: String,
}

impl Default for CandidateQuota {
    fn default() -> Self {
        Self {
            status: "dispatchable".to_string(),
            wall: "none".to_string(),
            used_percent: None,
            resets_at: None,
            freshness_secs: None,
            stale: false,
            source: "none".to_string(),
        }
    }
}

pub(super) fn candidate_quota(kind: AgentKind, custom_name: Option<&str>) -> CandidateQuota {
    quota_from(&availability(&kind, custom_name))
}

pub(super) fn quota_from(avail: &RouteAvailability) -> CandidateQuota {
    let tight = avail.probe.as_ref().and_then(|probe| tightest_window(&probe.windows));
    CandidateQuota {
        status: status_label(avail.status).to_string(),
        wall: wall_label(avail.wall).to_string(),
        used_percent: tight.map(|window| window.used_percent),
        resets_at: tight.and_then(|window| window.resets_at).map(|at| at.to_rfc3339()),
        freshness_secs: avail.probe.as_ref().map(|probe| probe.age.as_secs()),
        stale: avail.probe.as_ref().is_some_and(|probe| probe.stale),
        source: source_of(avail).to_string(),
    }
}

pub(super) struct NoteTarget<'a> {
    pub name: &'a str,
    pub kind: AgentKind,
    pub custom_name: Option<&'a str>,
}

pub(super) fn notes_for(
    targets: &[NoteTarget<'_>],
    urgency: TaskUrgency,
    recommended: Option<&str>,
) -> Vec<String> {
    targets
        .iter()
        .filter_map(|target| {
            let avail = availability(&target.kind, target.custom_name);
            route_note(target.name, &avail, urgency, recommended)
                .or_else(|| partial_note(target.name, target.kind, target.custom_name, &avail))
        })
        .collect()
}

fn route_note(
    name: &str,
    avail: &RouteAvailability,
    urgency: TaskUrgency,
    recommended: Option<&str>,
) -> Option<String> {
    match avail.status {
        RouteStatus::Held => Some(held_note(name, avail, urgency, recommended)),
        RouteStatus::Degraded => Some(degraded_note(name, avail)),
        RouteStatus::Dispatchable => None,
    }
}

fn held_note(
    name: &str,
    avail: &RouteAvailability,
    urgency: TaskUrgency,
    recommended: Option<&str>,
) -> String {
    let action = match urgency {
        TaskUrgency::Background => "; background work may wait",
        TaskUrgency::Urgent => "; switch agent immediately",
        TaskUrgency::Normal if recommended.is_some_and(|picked| picked != name) => "; skipped",
        TaskUrgency::Normal => "",
    };
    format!(
        "{name} {} ({}){}",
        avail.why,
        held_detail(avail),
        action
    )
}

fn held_detail(avail: &RouteAvailability) -> String {
    let wall = wall_label(avail.wall);
    match probe_detail(avail.probe.as_ref()) {
        Some(probe) => format!("{wall}; {probe}"),
        None => format!("{wall}; no probe"),
    }
}

fn degraded_note(name: &str, avail: &RouteAvailability) -> String {
    let used = avail
        .probe
        .as_ref()
        .and_then(|probe| tightest_window(&probe.windows))
        .map(|window| window.used_percent)
        .unwrap_or(0.0);
    let label = avail
        .probe
        .as_ref()
        .and_then(|probe| tightest_window(&probe.windows))
        .map(|window| window.label.as_str())
        .filter(|label| !label.is_empty())
        .unwrap_or("live");
    let resets = avail
        .probe
        .as_ref()
        .and_then(|probe| tightest_window(&probe.windows))
        .and_then(|window| window.resets_at)
        .map(|at| format!(" (resets {})", at.format("%H:%M")))
        .unwrap_or_default();
    format!("{name} degraded {used:.0}% of {label} window{resets}")
}

fn partial_note(
    name: &str,
    kind: AgentKind,
    custom_name: Option<&str>,
    avail: &RouteAvailability,
) -> Option<String> {
    if avail.status == RouteStatus::Held {
        return None;
    }
    let groups: Vec<String> = rate_limit::active_group_holds(&kind, custom_name)
        .into_iter()
        .map(|(group, _)| group)
        .collect();
    if groups.is_empty() {
        return None;
    }
    Some(format!(
        "{name} PARTIAL — {} held; agent still dispatchable",
        groups.join(", ")
    ))
}

fn probe_detail(probe: Option<&ProbeEvidence>) -> Option<String> {
    let probe = probe?;
    if !probe.ok {
        return Some("probe error".to_string());
    }
    let window = tightest_window(&probe.windows)?;
    let age = format_age(probe.age);
    let stale = if probe.stale { " STALE" } else { "" };
    let resets = window
        .resets_at
        .map(|at| format!(" resets {at}"))
        .unwrap_or_default();
    Some(format!(
        "probe {age} ago {:.2}% {}{resets}{stale}",
        window.used_percent, window.label
    ))
}

fn tightest_window(windows: &[WindowView]) -> Option<&WindowView> {
    windows
        .iter()
        .max_by(|left, right| {
            left.used_percent
                .partial_cmp(&right.used_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn format_age(age: std::time::Duration) -> String {
    let secs = age.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m", secs / 60)
    }
}

fn status_label(status: RouteStatus) -> &'static str {
    match status {
        RouteStatus::Held => "held",
        RouteStatus::Degraded => "degraded",
        RouteStatus::Dispatchable => "dispatchable",
    }
}

fn wall_label(wall: QuotaWall) -> &'static str {
    match wall {
        QuotaWall::Clock => "clock",
        QuotaWall::Windowed => "windowed",
        QuotaWall::Prepaid => "prepaid",
        QuotaWall::PlanChange => "plan_change",
        QuotaWall::Transient => "transient",
        QuotaWall::None => "none",
    }
}

fn source_of(avail: &RouteAvailability) -> &'static str {
    if avail.probe.is_some() {
        "probe"
    } else if avail.marker.is_some() {
        "marker"
    } else {
        "none"
    }
}
