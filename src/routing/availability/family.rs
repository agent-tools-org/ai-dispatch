// Family quota evidence shared by dispatch, fallback selection and continuation.
// Combines holds, dated live windows and dated untrusted recovery markers.
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
    crate::routing::rate_limit::marker_field(&text, "recovery_at: ")
        .as_deref()
        .and_then(crate::routing::rate_limit::parse_recovery_datetime)
        .is_some_and(|at| at > Local::now().naive_local())
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
