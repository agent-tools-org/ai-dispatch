// Hold classification and the single snapshot-override policy.
// Exports: StoredHold, stored_hold, classify_hold, wall_of, snapshot_overrides.
// Deps: rate_limit parse helpers, rate_limit_signatures, live_quota snapshots.

use chrono::{DateTime, Utc};
use std::path::Path;
use std::time::SystemTime;

use crate::live_quota;
use crate::rate_limit::{
    format_recovery, marker_field, marker_slug, parse_recovery_datetime, parse_recovery_time,
};
use crate::rate_limit_signatures::{
    match_quota_signature, match_quota_signature_for_agent, parse_relative_recovery, QuotaRecovery,
    QUOTA_SIGNATURES,
};
use crate::types::AgentKind;

use super::{ProbeEvidence, QuotaWall, WindowView};

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
pub(crate) fn classify_hold(message: &str) -> Hold {
    if let Some(stated) = parse_recovery_time(message) {
        return Hold::Until(stated);
    }
    if let Some(at) = parse_relative_recovery(message) {
        return Hold::Until(format_recovery(at));
    }
    match match_quota_signature(message) {
        Some((_, QuotaRecovery::NeedsHuman)) => Hold::NeedsHuman,
        Some((_, QuotaRecovery::Windowed)) => Hold::Windowed,
        Some((_, QuotaRecovery::After(minutes))) => Hold::Until(format_recovery(
            chrono::Local::now().naive_local() + chrono::Duration::minutes(minutes),
        )),
        None => Hold::Transient,
    }
}

/// Windowed signature match runs before `hold: manual`. Unmatched manual stays NeedsHuman.
pub(crate) fn stored_hold(content: &str, agent: &AgentKind) -> StoredHold {
    if let Some(recovery_at) = marker_field(content, "recovery_at: ")
        .as_deref()
        .and_then(parse_recovery_datetime)
    {
        return StoredHold::Until(recovery_at);
    }
    if stored_refusal_matches(content, agent, QuotaRecovery::Windowed) {
        return StoredHold::Windowed;
    }
    if marker_field(content, "hold: ").as_deref() == Some(MANUAL_HOLD)
        || stored_refusal_matches(content, agent, QuotaRecovery::NeedsHuman)
    {
        return StoredHold::NeedsHuman;
    }
    StoredHold::Transient
}

fn stored_refusal_matches(content: &str, agent: &AgentKind, want: QuotaRecovery) -> bool {
    content.lines().any(|line| {
        parse_recovery_time(line).is_none()
            && parse_relative_recovery(line).is_none()
            && match_quota_signature_for_agent(line, *agent) == Some(want)
    })
}

pub(crate) fn wall_of(agent: &AgentKind, content: &str) -> QuotaWall {
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
    content.lines().find_map(|line| {
        let lower = line.to_lowercase();
        QUOTA_SIGNATURES
            .iter()
            .find(|signature| signature.agent == *agent && lower.contains(signature.needle))
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

/// Agent-level uses every window. Group holds match `windows[].group` only.
/// Cursor Plan-label exception is PR-3.
pub(crate) fn relevant_windows(
    snapshot: &ProbeEvidence,
    _agent: &AgentKind,
    group: Option<&str>,
) -> Vec<WindowView> {
    match group {
        None => snapshot.windows.clone(),
        Some(group) => snapshot
            .windows
            .iter()
            .filter(|window| window_matches_group(window.group.as_deref(), group))
            .cloned()
            .collect(),
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

pub(crate) fn overrides_marker_at(agent: &AgentKind, marker_path: &Path) -> bool {
    let Some(cache_dir) = live_quota::cache_dir() else {
        return false;
    };
    overrides_marker_at_in_cache(agent, marker_path, &cache_dir)
}

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
