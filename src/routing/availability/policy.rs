// Hold classification and the single snapshot-override policy.
// Exports: StoredHold, stored_hold, classify_hold, wall_of, snapshot_overrides.
// Deps: rate_limit parse helpers, rate_limit_signatures, live_quota snapshots.

use chrono::{DateTime, Utc};
#[cfg(test)]
use std::path::Path;
use std::time::SystemTime;

use crate::routing::quota::live as live_quota;
use crate::routing::rate_limit::{
    format_recovery, marker_field, marker_slug, parse_recovery_datetime, parse_recovery_time,
};
use crate::routing::rate_limit::signatures::{
    match_quota_signature_for_agent, recovery_for_agent, QuotaRecovery,
    QUOTA_SIGNATURES,
};
use crate::types::AgentKind;

use super::{ProbeEvidence, QuotaWall, WindowView};

pub(super) fn stated_clock_why(content: Option<&str>, at: chrono::NaiveDateTime) -> String {
    let stated = content
        .and_then(|text| marker_field(text, "recovery_at: "))
        .unwrap_or_else(|| at.format("%b %d, %Y %I:%M %p").to_string());
    let capacity = content.and_then(|text| marker_field(text, "message: "))
        .is_some_and(|message| crate::routing::capacity::is_capacity(&message));
    format!("{}held until {stated}", if capacity { "provider capacity " } else { "" })
}

pub(super) fn cooldown_active(mtime: Option<SystemTime>) -> bool {
    mtime
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed.as_secs() < crate::routing::rate_limit::RATE_LIMIT_WINDOW_SECS)
}

/// Marker field value for a hold that only a person (or a dated window) can end.
pub(crate) const MANUAL_HOLD: &str = "manual";

const PLAN_CHANGE_NEEDLES: &[&str] = &["ineligibletier", "migrate to antigravity"];

/// Write-time class. Windowed writes the same bytes as NeedsHuman.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Hold {
    Until(String),
    NeedsHuman,
    Windowed,
    Transient,
}

/// Read-time class recovered from on-disk bytes + the current signature table.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StoredHold {
    Until(chrono::NaiveDateTime),
    Windowed,
    NeedsHuman,
    Transient,
}

/// A stated time always wins over the signature's class default.
pub(crate) fn classify_hold(message: &str, agent: AgentKind) -> Hold {
    if crate::routing::capacity::is_capacity(message) {
        return Hold::Until(format_recovery(chrono::Local::now().naive_local()
            + chrono::Duration::minutes(crate::routing::capacity::HOLD_MINUTES)));
    }
    if let Some(at) = recovery_for_agent(message, agent) {
        let stated = parse_recovery_time(message)
            .filter(|stated| parse_recovery_datetime(stated) == Some(at));
        return Hold::Until(stated.unwrap_or_else(|| format_recovery(at)));
    }
    match match_quota_signature_for_agent(message, agent) {
        Some(QuotaRecovery::NeedsHuman) => Hold::NeedsHuman,
        Some(QuotaRecovery::Windowed) => Hold::Windowed,
        Some(QuotaRecovery::After(minutes)) => Hold::Until(format_recovery(
            chrono::Local::now().naive_local() + chrono::Duration::minutes(minutes),
        )),
        None => Hold::Transient,
    }
}

/// Windowed signature match runs before `hold: manual`. Unmatched manual stays NeedsHuman.
/// A Windowed hold ends only when a dated live snapshot arrives; if this route
/// has no probe, that condition is unobservable, so the hold is human-cleared
/// rather than promising a snapshot that will never come.
pub(crate) fn stored_hold(content: &str, agent: &AgentKind) -> StoredHold {
    if marker_field(content, "source: ").as_deref() == Some("untrusted") {
        return StoredHold::Transient;
    }
    if let Some(recovery_at) = marker_field(content, "recovery_at: ")
        .as_deref()
        .and_then(parse_recovery_datetime)
    {
        return StoredHold::Until(recovery_at);
    }
    if stored_refusal_matches(content, agent, QuotaRecovery::Windowed) {
        if live_quota::snapshot(agent).is_some() {
            return StoredHold::Windowed;
        }
        return StoredHold::NeedsHuman;
    }
    if marker_field(content, "hold: ").as_deref() == Some(MANUAL_HOLD)
        || stored_refusal_matches(content, agent, QuotaRecovery::NeedsHuman)
    {
        return StoredHold::NeedsHuman;
    }
    StoredHold::Transient
}

fn stored_refusal_matches(content: &str, agent: &AgentKind, want: QuotaRecovery) -> bool {
    let signature_agent = crate::routing::rate_limit::stored_signature_agent(content, *agent);
    content.lines().any(|line| {
        recovery_for_agent(line, signature_agent).is_none()
            && match_quota_signature_for_agent(line, signature_agent) == Some(want)
    })
}

pub(crate) fn wall_of(agent: &AgentKind, content: &str) -> QuotaWall {
    if marker_field(content, "message: ").is_some_and(|message| crate::routing::capacity::is_capacity(&message)) {
        return QuotaWall::Transient;
    }
    match stored_hold(content, agent) {
        StoredHold::Until(_) => QuotaWall::Clock,
        StoredHold::Windowed => QuotaWall::Windowed,
        StoredHold::NeedsHuman => match signature_needle(agent, content) {
            Some(needle) if PLAN_CHANGE_NEEDLES.contains(&needle) => QuotaWall::PlanChange,
            _ => QuotaWall::Prepaid,
        },
        StoredHold::Transient => QuotaWall::Transient,
    }
}

fn signature_needle(agent: &AgentKind, content: &str) -> Option<&'static str> {
    let signature_agent = crate::routing::rate_limit::stored_signature_agent(content, *agent);
    content.lines().find_map(|line| {
        let lower = line.to_lowercase();
        QUOTA_SIGNATURES
            .iter()
            .find(|signature| signature.agent == signature_agent && lower.contains(signature.needle))
            .map(|signature| signature.needle)
    })
}

/// THE only override policy. No age cap — v10.19 newer-than-marker.
pub(crate) fn snapshot_overrides(
    hold: &StoredHold,
    snapshot: &ProbeEvidence,
    marker_mtime: SystemTime,
    relevant: &[WindowView],
) -> bool {
    if !snapshot.ok || snapshot.fetched_at <= DateTime::<Utc>::from(marker_mtime) {
        return false;
    }
    if relevant.is_empty() {
        return false;
    }
    if !relevant
        .iter()
        .all(|window| (0.0..100.0).contains(&window.used_percent))
    {
        return false;
    }
    match hold {
        StoredHold::Until(_) | StoredHold::Transient => true,
        StoredHold::Windowed => relevant.iter().any(|window| window.resets_at.is_some()),
        StoredHold::NeedsHuman => false,
    }
}

/// Agent-level uses every window. Group field wins when present.
/// Cursor premium without a group field matches the Plan label only.
pub(crate) fn relevant_windows(
    snapshot: &ProbeEvidence,
    agent: &AgentKind,
    group: Option<&str>,
) -> Vec<WindowView> {
    match group {
        None => snapshot.windows.clone(),
        Some(group) => {
            let by_field: Vec<_> = snapshot
                .windows
                .iter()
                .filter(|window| window_matches_group(window.group.as_deref(), group))
                .cloned()
                .collect();
            if !by_field.is_empty() {
                return by_field;
            }
            if *agent == AgentKind::Cursor && group.eq_ignore_ascii_case("premium") {
                return snapshot
                    .windows
                    .iter()
                    .filter(|window| window.label.eq_ignore_ascii_case("plan"))
                    .cloned()
                    .collect();
            }
            Vec::new()
        }
    }
}

fn window_matches_group(window_group: Option<&str>, asked: &str) -> bool {
    match window_group {
        Some(group) if group == asked => true,
        // One probe pool: aidbar writes a single claude-gpt window for Claude+GPT.
        Some("claude-gpt") if matches!(asked, "claude" | "gpt-oss") => true,
        _ => false,
    }
}

#[cfg(test)]
pub(crate) fn overrides_marker_at_in_cache(
    agent: &AgentKind,
    marker_path: &Path,
    cache_dir: &Path,
) -> bool {
    let Ok(content) = std::fs::read_to_string(marker_path) else {
        return false;
    };
    let Ok(marker_mtime) = std::fs::metadata(marker_path).and_then(|meta| meta.modified()) else {
        return false;
    };
    let Some(snapshot) = live_quota::snapshot_in_cache(agent, cache_dir) else {
        return false;
    };
    let group = group_from_marker_path(marker_path);
    let relevant = relevant_windows(&snapshot, agent, group.as_deref());
    snapshot_overrides(
        &stored_hold(&content, agent),
        &snapshot,
        marker_mtime,
        &relevant,
    )
}

#[cfg(test)]
fn group_from_marker_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("rate-limit-")?;
    rest.split_once("--").map(|(_, group)| group.to_string())
}

pub(crate) fn format_hold_end_for(
    hold: &StoredHold,
    agent: &AgentKind,
    custom_name: Option<&str>,
    recovery_at: Option<&str>,
) -> String {
    let slug = marker_slug(agent, custom_name);
    match hold {
        StoredHold::Until(_) => format!("resets {}", recovery_at.unwrap_or("unknown")),
        StoredHold::Windowed => {
            let provider = live_quota::provider_name(agent).unwrap_or(agent.as_str());
            format!(
                "until a dated {provider} snapshot with headroom (or `aid config clear-limit {slug}`)"
            )
        }
        StoredHold::NeedsHuman => {
            format!("held until cleared with `aid config clear-limit {slug}`")
        }
        StoredHold::Transient => "cooling down".to_string(),
    }
}

#[cfg(test)]
#[path = "policy_scope_tests.rs"]
mod scope_tests;
