// Existing rate-limit marker clearing and discovery extracted from the public facade.
// Deps: parent marker, route availability and signature helpers.

use super::*;

/// Clear a marker only when it predates `task_start`.
///
/// A successful task is not evidence that a provider has quota. When the same
/// run just observed a refusal and recorded it, clearing on "success" erases the
/// outage microseconds after it was captured and hands routing back a provider
/// that is out — which is how a marker written by `record_quota_exhaustion`
/// survived the watcher and then died in `handle_done_postprocess`.
///
/// Returns true when a marker was actually removed.
pub fn clear_rate_limit_if_stale(
    agent: &AgentKind,
    custom_name: Option<&str>,
    task_start: DateTime<Local>,
) -> bool {
    let path = marker_path(agent, custom_name);
    let written_after_start = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .map(|modified| DateTime::<Local>::from(modified) >= task_start)
        .unwrap_or(false);
    if written_after_start {
        return false;
    }
    clear_rate_limit(agent, custom_name)
}

pub fn clear_group_rate_limit_if_stale(
    agent: &AgentKind,
    custom_name: Option<&str>,
    group: &str,
    task_start: DateTime<Local>,
) -> bool {
    let path = group_marker_path(agent, custom_name, group);
    let written_after_start = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .map(|modified| DateTime::<Local>::from(modified) >= task_start)
        .unwrap_or(false);
    if written_after_start {
        return false;
    }
    clear_group_rate_limit(agent, custom_name, group)
}

pub fn clear_rate_limit_for_model_if_stale(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
    task_start: DateTime<Local>,
) -> bool {
    let mut cleared = clear_rate_limit_if_stale(agent, custom_name, task_start);
    cleared |= clear_group_rate_limit_if_stale(
        agent,
        custom_name,
        &crate::routing::capacity::group(model),
        task_start,
    );
    if let Some(group) = crate::agent::model_group::model_group(*agent, model) {
        if clear_group_rate_limit_if_stale(agent, custom_name, group, task_start) {
            cleared = true;
        }
    }
    cleared
}

pub fn clear_rate_limit_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> bool {
    let mut cleared = clear_rate_limit(agent, custom_name);
    cleared |= clear_group_rate_limit(agent, custom_name, &crate::routing::capacity::group(model));
    if let Some(group) = crate::agent::model_group::model_group(*agent, model) {
        if clear_group_rate_limit(agent, custom_name, group) {
            cleared = true;
        }
    }
    cleared
}

pub fn clear_rate_limit(agent: &AgentKind, custom_name: Option<&str>) -> bool {
    fs::remove_file(marker_path(agent, custom_name)).is_ok()
}

pub fn clear_all_rate_limits_for_agent(agent: &AgentKind, custom_name: Option<&str>) -> bool {
    let mut cleared = clear_rate_limit(agent, custom_name);
    for (group, _) in crate::agent::model_group::groups_for_agent(*agent) {
        if clear_group_rate_limit(agent, custom_name, group) {
            cleared = true;
        }
    }
    for (group, _) in discovered_group_markers(agent, custom_name) {
        if clear_group_rate_limit(agent, custom_name, &group) {
            cleared = true;
        }
    }
    cleared
}

pub(super) fn discovered_group_markers(
    agent: &AgentKind,
    custom_name: Option<&str>,
) -> Vec<(String, PathBuf)> {
    let prefix = format!("rate-limit-{}--", marker_slug(agent, custom_name));
    let Ok(entries) = fs::read_dir(aid_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let group = name.strip_prefix(&prefix)?.to_string();
            if *agent != AgentKind::OpenCode && !group.starts_with("capacity-") {
                return None;
            }
            (!group.is_empty()).then_some((group, entry.path()))
        })
        .collect()
}

pub fn is_rate_limited(agent: &AgentKind, custom_name: Option<&str>) -> bool {
    matches!(availability(agent, custom_name).status, RouteStatus::Held)
}

/// Currently held agents as `(display name, message)`. Includes registered
/// custom agents, each under its own marker.
pub fn rate_limited_agents() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .filter_map(|agent| {
            let info = get_rate_limit_info(&agent, None)?;
            is_rate_limited(&agent, None)
                .then(|| (agent.as_str().to_string(), info.message.unwrap_or_default()))
        })
        .collect();
    for config in crate::agent::registry::list_custom_agents() {
        let name = config.id.as_str();
        if let Some(info) = get_rate_limit_info(&AgentKind::Custom, Some(name))
            && is_rate_limited(&AgentKind::Custom, Some(name))
        {
            out.push((config.id, info.message.unwrap_or_default()));
        }
    }
    out
}
