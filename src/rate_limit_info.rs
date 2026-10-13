// Existing rate-limit hold display and enumeration extracted from the public facade.
// Deps: parent marker, route availability and signature helpers.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct RateLimitInfo {
    pub recovery_at: Option<String>,
    pub message: Option<String>,
    /// True when only `aid config clear-limit` ends this hold. Distinguishes a
    /// spent balance from a transient refusal, which also has no recovery time
    /// but expires on its own.
    pub needs_human: bool,
    /// Full marker bytes. `message` is only the first `message:` line; grok's
    /// needle lives later in a JSON body, so display must classify this field.
    pub(crate) marker: String,
}

pub fn recovery_datetime(agent: &AgentKind, custom_name: Option<&str>) -> Option<NaiveDateTime> {
    let recovery_at = get_rate_limit_info(agent, custom_name)?.recovery_at?;
    parse_recovery_datetime(&recovery_at)
}

pub fn get_rate_limit_info(agent: &AgentKind, custom_name: Option<&str>) -> Option<RateLimitInfo> {
    let path = marker_path(agent, custom_name);
    let content = fs::read_to_string(&path).ok()?;
    Some(info_from_marker_content(&content, agent))
}

fn info_from_marker_content(content: &str, agent: &AgentKind) -> RateLimitInfo {
    RateLimitInfo {
        recovery_at: marker_field(content, "recovery_at: "),
        message: marker_field(content, "message: "),
        needs_human: matches!(stored_hold(content, agent), StoredHold::NeedsHuman),
        marker: content.to_string(),
    }
}

/// Live model-group holds for an agent. Empty when every group is clear, and
/// independent of the agent-level marker — a held premium pool does not belong
/// in `is_rate_limited`.
pub fn active_group_holds(
    agent: &AgentKind,
    custom_name: Option<&str>,
) -> Vec<(String, RateLimitInfo)> {
    let mut groups: Vec<(String, PathBuf)> = crate::agent::model_group::groups_for_agent(*agent)
        .iter()
        .map(|(group, _)| {
            (
                (*group).to_string(),
                group_marker_path(agent, custom_name, group),
            )
        })
        .collect();
    groups.extend(discovered_group_markers(agent, custom_name));
    groups
        .into_iter()
        .filter_map(|(group, path)| {
            if !matches!(
                availability_for_group(agent, custom_name, &group).status,
                RouteStatus::Held
            ) {
                return None;
            }
            let content = fs::read_to_string(&path).ok()?;
            Some((group, info_from_marker_content(&content, agent)))
        })
        .collect()
}

/// How a live hold ends, for display. Never invents a reset time aid did not
/// observe — a missing recovery is either a person-clear or a short cooldown.
pub fn format_hold_end(
    agent: &AgentKind,
    custom_name: Option<&str>,
    info: &RateLimitInfo,
) -> String {
    if info.needs_human {
        return format_needs_human_hold(agent, custom_name, info.message.as_deref());
    }
    if let Some(at) = info.recovery_at.as_deref() {
        return format!("resets {at}");
    }
    format_hold_end_for(&stored_hold(&info.marker, agent), agent, custom_name, None)
}

/// Shared NeedsHuman text for `aid agent list`, the session-start hook, and
/// `aid doctor`. Windowed/After holds do not go through this.
pub(crate) fn format_needs_human_hold(
    agent: &AgentKind,
    custom_name: Option<&str>,
    message: Option<&str>,
) -> String {
    let slug = marker_slug(agent, custom_name);
    let first = message
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty());
    match first {
        Some(line) => {
            format!("needs human: {line} — fix, then `aid config clear-limit {slug}`")
        }
        None => format!("needs human — fix, then `aid config clear-limit {slug}`"),
    }
}
