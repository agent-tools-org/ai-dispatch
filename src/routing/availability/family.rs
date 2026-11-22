// Family quota evidence shared by dispatch, fallback selection and continuation.
// Combines holds, dated live windows and dated untrusted recovery markers;
// a later reading that shows the unit with headroom overrides an untrusted marker.
use super::*;

pub(crate) fn family_is_spent(agent: AgentKind, custom: Option<&str>, family: &str) -> bool {
    if availability_for_group(&agent, custom, family).status == RouteStatus::Held {
        return true;
    }
    let probe = live_quota::snapshot(&agent);
    if probe.as_ref().is_some_and(|probe| probe_is_spent(probe, agent, family)) {
        return true;
    }
    let path = group_marker_path(&agent, custom, family);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    if crate::routing::rate_limit::marker_field(&text, "source: ").as_deref() != Some("untrusted") {
        return false;
    }
    let in_force = crate::routing::rate_limit::marker_field(&text, "recovery_at: ")
        .as_deref()
        .and_then(crate::routing::rate_limit::parse_recovery_datetime)
        .is_some_and(|at| at > Local::now().naive_local());
    // A reading taken after the refusal that shows this unit with headroom
    // outranks the refusal's guessed recovery time. The probe cache and the
    // marker are written on this host, so both timestamps share one clock.
    let written = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    in_force
        && !probe.is_some_and(|probe| {
            written.is_some_and(|at| probe.fetched_at > chrono::DateTime::<chrono::Utc>::from(at))
                && probe_shows_headroom(&probe, agent, family)
        })
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

/// True when a fresh probe meters `family` itself and none of its windows for
/// the unit is full. Argon also draws on general Gemini windows, so only an
/// Argon-labelled window can vouch for it; a full window always vetoes.
fn probe_shows_headroom(probe: &ProbeEvidence, agent: AgentKind, family: &str) -> bool {
    let relevant = relevant_windows(probe, &agent, Some(family));
    let meters_unit = relevant.iter().any(|window| {
        agent != AgentKind::Antigravity
            || family != "gemini-4-argon"
            || crate::agent::model_group::agy_unit(&window.label) == family
    });
    probe.ok
        && !probe.stale
        && meters_unit
        && relevant.iter().all(|window| window.used_percent < 100.0)
}

#[cfg(test)]
#[path = "family_tests.rs"]
mod tests;
