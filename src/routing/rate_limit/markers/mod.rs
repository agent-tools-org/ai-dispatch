// Existing rate-limit marker writes extracted from the public facade.
// Deps: parent marker, route availability and signature helpers.

use super::*;

/// Marker file slug under `~/.aid/rate-limit-*`.
///
/// Built-ins keep `AgentKind::as_str()` so live files like `rate-limit-codex`
/// stay put. A custom agent uses its own id — otherwise every custom shares
/// one `rate-limit-custom` hold.
pub fn marker_slug<'a>(agent: &'a AgentKind, custom_name: Option<&'a str>) -> &'a str {
    match (agent, custom_name) {
        (AgentKind::Custom, Some(name)) if !name.is_empty() => name,
        _ => agent.as_str(),
    }
}

pub(crate) fn marker_path(agent: &AgentKind, custom_name: Option<&str>) -> PathBuf {
    #[cfg(test)]
    assert_marker_path_isolated();
    aid_dir().join(format!("rate-limit-{}", marker_slug(agent, custom_name)))
}

/// Marker for one model group of an agent whose plan meters families
/// separately. agy's gemini allowance can be exhausted while its claude
/// allowance still serves; a per-agent marker would strand the working one.
pub(crate) fn group_marker_path(
    agent: &AgentKind,
    custom_name: Option<&str>,
    group: &str,
) -> PathBuf {
    #[cfg(test)]
    assert_marker_path_isolated();
    let group = group.to_ascii_lowercase();
    aid_dir().join(format!(
        "rate-limit-{}--{}",
        marker_slug(agent, custom_name),
        group
    ))
}

pub fn mark_group_rate_limited(
    agent: &AgentKind,
    custom_name: Option<&str>,
    group: &str,
    message: &str,
) {
    let provider = (*agent == AgentKind::OpenCode).then_some(group);
    write_marker(
        &group_marker_path(agent, custom_name, group),
        message,
        provider,
        *agent,
        super::identity::signature_agent(*agent, custom_name),
    );
}

pub fn is_group_rate_limited(agent: &AgentKind, custom_name: Option<&str>, group: &str) -> bool {
    matches!(
        availability_for_group(agent, custom_name, group).status,
        RouteStatus::Held
    )
}

pub fn clear_group_rate_limit(agent: &AgentKind, custom_name: Option<&str>, group: &str) -> bool {
    fs::remove_file(group_marker_path(agent, custom_name, group)).is_ok()
}

pub fn mark_rate_limited(agent: &AgentKind, custom_name: Option<&str>, message: &str) {
    mark_rate_limited_for_signature(
        agent, custom_name, super::identity::signature_agent(*agent, custom_name), message,
    );
}

/// Record a refusal when the caller has no model in hand — a stderr line, a
/// stream error event, a failed task's captured output.
///
/// The refusal can still name the tier it exhausted even when the model is
/// unknown, and marking the whole agent for a tier refusal takes a route out
/// that is still serving: cursor's "You're out of usage. Switch to Auto" went
/// through `mark_rate_limited`, so `is_rate_limited(Cursor, None)` became true and
/// `auto` — the tier the message itself points at — stopped being dispatchable.
pub fn mark_rate_limited_for_message(agent: &AgentKind, custom_name: Option<&str>, message: &str) {
    mark_rate_limited_for_model(agent, custom_name, None, message);
}

/// Attribute the hold with `quota_group_for_mark`. That is not simply
/// "dispatched route first": for an agent whose refusals name their own tier,
/// the message decides and the model may not narrow it. See the helper.
pub fn mark_rate_limited_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
    message: &str,
) {
    if crate::routing::capacity::is_capacity(message) {
        mark_group_rate_limited(
            agent,
            custom_name,
            &crate::routing::capacity::group(model),
            message,
        );
        return;
    }
    match quota_group_for_mark(*agent, model, message, None) {
        Some(group) => mark_group_rate_limited(agent, custom_name, group, message),
        None => mark_rate_limited(agent, custom_name, message),
    }
}

pub fn mark_rate_limited_for_value(
    agent: &AgentKind,
    custom_name: Option<&str>,
    evidence: &serde_json::Value,
    message: &str,
) {
    mark_rate_limited_for_model_value(agent, custom_name, None, evidence, message);
}

/// Same attribution as `mark_rate_limited_for_model`, with a parsed refusal
/// envelope available for providers that name the exhausted unit in structured
/// fields. `quota_group_for_mark` owns the precedence; do not reorder it here.
pub fn mark_rate_limited_for_model_value(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
    evidence: &serde_json::Value,
    message: &str,
) {
    match quota_group_for_mark(*agent, model, message, Some(evidence)) {
        Some(group) => mark_group_rate_limited(agent, custom_name, group, message),
        None => mark_rate_limited(agent, custom_name, message),
    }
}

/// For droid the refusal is the authority: `standard usage` marks `standard`,
/// `droid core usage` marks `core`; anything else is agent-wide. A dispatched
/// model must not narrow a 402 that named no tier — `model_group(Droid, Some(_))`
/// is never None, so model-first would hold only that pool. Other agents still
/// prefer the dispatched model, then the refusal.
fn quota_group_for_mark<'a>(
    agent: AgentKind,
    model: Option<&'a str>,
    message: &'a str,
    evidence: Option<&'a serde_json::Value>,
) -> Option<&'a str> {
    if agent == AgentKind::Droid {
        return crate::agent::model_group::group_from_refusal(agent, message);
    }
    if let Some(name) = model
        && let Some(group) = crate::agent::model_group::model_group(agent, Some(name))
    {
        return Some(group);
    }
    evidence
        .and_then(|value| crate::agent::model_group::group_from_refusal_value(agent, value))
        .or_else(|| crate::agent::model_group::group_from_refusal(agent, message))
}

pub(crate) fn format_recovery(at: NaiveDateTime) -> String {
    at.format("%b %d, %Y %I:%M %p").to_string()
}

fn write_marker(
    path: &std::path::Path,
    message: &str,
    provider: Option<&str>,
    agent: AgentKind,
    signature_agent: AgentKind,
) {
    write_marker_with_hold(
        path,
        message,
        provider,
        agent,
        signature_agent,
        classify_hold(message, signature_agent),
    );
}

fn write_marker_with_hold(
    path: &std::path::Path,
    message: &str,
    provider: Option<&str>,
    agent: AgentKind,
    signature_agent: AgentKind,
    hold: Hold,
) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if preserves_dated_hold(path, provider, agent, &hold) {
        return;
    }
    let (recovery_at, hold_line) = match hold {
        Hold::Until(at) => (at, String::new()),
        Hold::NeedsHuman | Hold::Windowed => (String::new(), format!("hold: {MANUAL_HOLD}\n")),
        Hold::Transient => (String::new(), String::new()),
    };
    let provider_line = format!("provider: {}\n", provider.unwrap_or("unknown"));
    let signature_line = format!("signature_agent: {}\n", signature_agent.as_str());
    let content = format!(
        "recovery_at: {recovery_at}\n{hold_line}{provider_line}{signature_line}message: {message}\n",
    );
    let _ = fs::write(path, content);
}

/// Read a `key: value` field from a marker, treating an empty value as absent.
pub(crate) fn marker_field(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Unsplit output can preserve a refusal excerpt, but cannot establish a long hold.
pub(crate) fn mark_untrusted_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
    message: &str,
) {
    let path = match quota_group_for_mark(*agent, model, message, None) {
        Some(group) => group_marker_path(agent, custom_name, group),
        None => marker_path(agent, custom_name),
    };
    if let Ok(content) = fs::read_to_string(&path)
        && !matches!(stored_hold(&content, agent), StoredHold::Transient)
    {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let excerpt = message.lines().take(1).collect::<String>();
    let _ = fs::write(path, format!("source: untrusted\nmessage: {excerpt}\n"));
}

fn preserves_dated_hold(
    path: &std::path::Path,
    provider: Option<&str>,
    agent: AgentKind,
    incoming: &Hold,
) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    if marker_field(&content, "provider: ").as_deref() != Some(provider.unwrap_or("unknown")) {
        return false;
    }
    let StoredHold::Until(existing) = stored_hold(&content, &agent) else {
        return false;
    };
    if existing <= Local::now().naive_local() {
        return false;
    }
    match incoming {
        Hold::Until(at) => parse_recovery_datetime(at).is_some_and(|at| existing > at),
        Hold::Transient => true,
        Hold::NeedsHuman | Hold::Windowed => false,
    }
}

pub(crate) fn mark_rate_limited_for_signature(
    agent: &AgentKind,
    custom_name: Option<&str>,
    signature_agent: AgentKind,
    message: &str,
) {
    let signature_agent = if *agent == AgentKind::Custom { signature_agent } else { *agent };
    write_marker(&marker_path(agent, custom_name), message, None, *agent, signature_agent);
}

#[cfg(test)]
pub(crate) fn mark_rate_limited_until(
    agent: &AgentKind,
    custom_name: Option<&str>,
    reset: NaiveDateTime,
    message: &str,
) {
    write_marker_with_hold(
        &marker_path(agent, custom_name),
        message,
        None,
        *agent,
        super::identity::signature_agent(*agent, custom_name),
        Hold::Until(format_recovery(reset)),
    );
}
