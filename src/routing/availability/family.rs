// Family quota evidence shared by dispatch, fallback selection and continuation.
// Combines holds, dated live windows and duration-bearing untrusted markers.
use super::*;

pub(crate) fn family_is_spent(agent: AgentKind, custom: Option<&str>, family: &str) -> bool {
    if availability_for_group(&agent, custom, family).status == RouteStatus::Held {
        return true;
    }
    if let Some(probe) = live_quota::snapshot(&agent)
        && probe_is_spent(&probe, agent, family)
    {
        return true;
    }
    let path = group_marker_path(&agent, custom, family);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    if crate::routing::rate_limit::marker_field(&text, "source: ").as_deref() != Some("untrusted") {
        return false;
    }
    if let Some(at) = crate::routing::rate_limit::marker_field(&text, "recovery_at: ")
        .as_deref()
        .and_then(crate::routing::rate_limit::parse_recovery_datetime)
    {
        return at > Local::now().naive_local();
    }
    let Some(at) = crate::routing::rate_limit::signatures::recovery_for_agent(&text, agent) else {
        return false;
    };
    let duration = at - Local::now().naive_local();
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|mtime| mtime.elapsed().ok())
        .is_some_and(|age| age.as_secs() < duration.num_seconds().max(0) as u64)
}

fn probe_is_spent(probe: &ProbeEvidence, agent: AgentKind, family: &str) -> bool {
    let relevant = relevant_windows(probe, &agent, Some(family));
    probe.ok
        && !probe.stale
        && probe.windows.iter().any(|window| {
            relevant.contains(window)
                && window.used_percent >= 100.0
                && window.resets_at.is_some_and(|at| at > chrono::Utc::now())
        })
}

#[cfg(test)]
#[path = "family_tests.rs"]
mod tests;
