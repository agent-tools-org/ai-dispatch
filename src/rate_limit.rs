// Rate-limit detection: marks agents as rate-limited when quota errors occur.
// A marker is held by a stated reset time, by a person, or by a short cooldown.
// Exports: mark_rate_limited{,_for_message}, is_rate_limited,
// dispatch_blocking_hold, get_rate_limit_info, active_group_holds,
// format_hold_end, clear_*.

use crate::paths::aid_dir;
use crate::rate_limit_signatures::QuotaRecovery;
use crate::types::AgentKind;
use chrono::{DateTime, Local, NaiveDateTime};
#[cfg(test)]
use chrono::{Datelike, Timelike};
use std::fs;
use std::path::PathBuf;

/// Cooldown for a refusal that named no reset time and matched no signature —
/// a bare 429 or 402 seen on stderr. All we know is that it just happened, so
/// the route is stepped over briefly and then tried again. Anything longer
/// would be an invented outage for a route that is probably still serving.
const RATE_LIMIT_WINDOW_SECS: u64 = 300;

/// Marker field value for a hold that only a person can end.
const MANUAL_HOLD: &str = "manual";


#[cfg(test)]
fn assert_marker_path_isolated() {
    crate::paths::assert_aid_home_isolated("rate_limit::marker_path");
}

/// Resolve a CLI agent name into kind + optional custom id for marker paths.
pub fn resolve_agent(name: &str) -> (AgentKind, Option<&str>) {
    match AgentKind::parse_str(name) {
        Some(kind) => (kind, None),
        None => (AgentKind::Custom, Some(name)),
    }
}

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

fn marker_path(agent: &AgentKind, custom_name: Option<&str>) -> PathBuf {
    #[cfg(test)]
    assert_marker_path_isolated();
    aid_dir().join(format!("rate-limit-{}", marker_slug(agent, custom_name)))
}

/// Marker for one model group of an agent whose plan meters families
/// separately. agy's gemini allowance can be exhausted while its claude
/// allowance still serves; a per-agent marker would strand the working one.
fn group_marker_path(agent: &AgentKind, custom_name: Option<&str>, group: &str) -> PathBuf {
    #[cfg(test)]
    assert_marker_path_isolated();
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
    write_marker(&group_marker_path(agent, custom_name, group), message, provider);
}

pub fn is_group_rate_limited(agent: &AgentKind, custom_name: Option<&str>, group: &str) -> bool {
    marker_is_active(&group_marker_path(agent, custom_name, group), agent)
}

pub fn clear_group_rate_limit(agent: &AgentKind, custom_name: Option<&str>, group: &str) -> bool {
    fs::remove_file(group_marker_path(agent, custom_name, group)).is_ok()
}

pub fn mark_rate_limited(agent: &AgentKind, custom_name: Option<&str>, message: &str) {
    write_marker(&marker_path(agent, custom_name), message, None);
}

/// Record a refusal when the caller has no model in hand — a stderr line, a
/// stream error event, a failed task's captured output.
///
/// The refusal can still name the tier it exhausted even when the model is
/// unknown, and marking the whole agent for a tier refusal takes a route out
/// that is still serving: cursor's "You're out of usage. Switch to Auto" went
/// through `mark_rate_limited`, so `is_rate_limited(Cursor, None)` became true and
/// `auto` — the tier the message itself points at — stopped being dispatchable.
pub fn mark_rate_limited_for_message(
    agent: &AgentKind,
    custom_name: Option<&str>,
    message: &str,
) {
    match crate::agent::model_group::group_from_refusal(*agent, message) {
        Some(group) => mark_group_rate_limited(agent, custom_name, group, message),
        None => mark_rate_limited(agent, custom_name, message),
    }
}

/// What is holding a marker open. These are three different facts and
/// collapsing any two of them loses a route in one direction or the other.
enum Hold {
    /// The provider stated when it comes back, or its signature says how long
    /// that class of window runs. Held until that instant.
    Until(String),
    /// Only a person ends it — a top-up, a plan change, an admin raising a
    /// limit. Held until `aid config clear-limit <agent>`.
    NeedsHuman,
    /// Neither: no reset time and no signature. Held for a bounded cooldown.
    Transient,
}

/// Decide, once at write time, what will end this refusal.
///
/// A stated time always wins over the signature's class default: a copilot
/// message that does name its reset date is held to that date, not to a person.
fn classify_hold(message: &str) -> Hold {
    if let Some(stated) = parse_recovery_time(message) {
        return Hold::Until(stated);
    }
    if let Some(at) = crate::rate_limit_signatures::parse_relative_recovery(message) {
        return Hold::Until(format_recovery(at));
    }
    match crate::rate_limit_signatures::match_quota_signature(message) {
        Some((_, QuotaRecovery::NeedsHuman)) => Hold::NeedsHuman,
        Some((_, QuotaRecovery::After(minutes))) => Hold::Until(format_recovery(
            Local::now().naive_local() + chrono::Duration::minutes(minutes),
        )),
        None => Hold::Transient,
    }
}

fn format_recovery(at: NaiveDateTime) -> String {
    at.format("%b %d, %Y %I:%M %p").to_string()
}

fn write_marker(path: &std::path::Path, message: &str, provider: Option<&str>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let (recovery_at, hold_line) = match classify_hold(message) {
        Hold::Until(at) => (at, String::new()),
        Hold::NeedsHuman => (String::new(), format!("hold: {MANUAL_HOLD}\n")),
        Hold::Transient => (String::new(), String::new()),
    };
    let provider_line = format!("provider: {}\n", provider.unwrap_or("unknown"));
    let content = format!("recovery_at: {recovery_at}\n{hold_line}{provider_line}message: {message}\n");
    let _ = fs::write(path, content);
}

/// Read a `key: value` field from a marker, treating an empty value as absent.
fn marker_field(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// What is holding a marker that is already on disk. The read-side counterpart
/// of `Hold`: the stated time has been parsed, so callers compare instants.
enum StoredHold {
    Until(NaiveDateTime),
    NeedsHuman,
    Transient,
}

/// Classify a marker file's contents.
///
/// A marker with no parseable reset time is not automatically permanent. Before
/// the hold classes existed this fell straight through to "still limited",
/// which meant one transient 429 caught on stderr — the generic path writes
/// exactly such a marker — took a route out until someone ran
/// `aid config clear-limit`. That is the same defect as an outage going
/// unrecorded, pointing the other way: a route that still serves, written off.
///
/// Markers written before the hold classes existed carry no `hold:` line at all,
/// and the human-ended ones carry no reset time either — copilot's and grok's
/// live markers are both in that shape. Rather than rewrite files this version
/// did not author, the stored refusal text is re-read: it is the same evidence
/// write-time classification uses. Only the `NeedsHuman` verdict is taken from
/// it, because an `After` window read here would be measured from read time and
/// so could never elapse.
fn stored_hold(content: &str, agent: &AgentKind) -> StoredHold {
    if let Some(recovery_at) =
        marker_field(content, "recovery_at: ").as_deref().and_then(parse_recovery_datetime)
    {
        return StoredHold::Until(recovery_at);
    }
    if marker_field(content, "hold: ").as_deref() == Some(MANUAL_HOLD)
        || stored_refusal_needs_a_person(content, agent)
    {
        return StoredHold::NeedsHuman;
    }
    StoredHold::Transient
}

pub(crate) fn live_quota_can_override(content: &str, agent: &AgentKind) -> bool {
    !matches!(stored_hold(content, agent), StoredHold::NeedsHuman)
}

/// Whether the refusal a marker recorded is one only a person ends.
///
/// Matched line by line: grok's marker wraps its refusal in a multi-line JSON
/// body, so reading only the first `message:` line misses it.
///
/// Scoped to the agent whose marker this is. A marker is aid's record of what
/// *one* provider said, so a needle another provider owns is not evidence about
/// this one — `~/.aid/rate-limit-claude`, written on 2026-08-07 from an agent's
/// own message quoting this crate's signature table, held claude open on
/// opencode's `insufficient balance`. The write side can no longer produce such
/// a marker (`quota_channel`), but markers already on disk predate that and are
/// still read here.
fn stored_refusal_needs_a_person(content: &str, agent: &AgentKind) -> bool {
    content.lines().any(|line| {
        parse_recovery_time(line).is_none()
            && crate::rate_limit_signatures::parse_relative_recovery(line).is_none()
            && crate::rate_limit_signatures::match_quota_signature_for_agent(line, *agent)
                == Some(QuotaRecovery::NeedsHuman)
    })
}

/// Shared liveness check for a marker file.
fn marker_is_active(path: &std::path::Path, agent: &AgentKind) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    match stored_hold(&content, agent) {
        StoredHold::Until(recovery_at) => recovery_at > Local::now().naive_local(),
        StoredHold::NeedsHuman => true,
        StoredHold::Transient => within_cooldown_window(path),
    }
}

/// How a live hold ends, phrased for the caller who just chose this agent —
/// `None` when nothing should stop the dispatch.
///
/// `aid run` gated on the presence of a recovery time, which was wrong in both
/// directions: a marker whose stated time had already passed still diverted the
/// run, and a refusal only a person can end carries no time at all, so dispatch
/// walked straight into an account that cannot serve.
///
/// The bounded transient cooldown is deliberately not a gate. It is short enough
/// that moving the caller off the agent they asked for costs more than the wait,
/// and gating on it was never the previous behaviour either.
pub fn dispatch_blocking_hold(agent: &AgentKind, custom_name: Option<&str>) -> Option<String> {
    dispatch_blocking_hold_at_path(&marker_path(agent, custom_name), agent, custom_name)
}

pub fn dispatch_blocking_hold_for_model(
    agent: &AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> Option<String> {
    let group = crate::agent::model_group::model_group(*agent, model)?;
    dispatch_blocking_hold_at_path(
        &group_marker_path(agent, custom_name, group),
        agent,
        custom_name,
    )
}

fn dispatch_blocking_hold_at_path(
    path: &std::path::Path,
    agent: &AgentKind,
    custom_name: Option<&str>,
) -> Option<String> {
    if !marker_is_active(path, agent) || crate::live_quota::overrides_marker(agent, path) {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    match stored_hold(&content, agent) {
        StoredHold::Until(recovery_at) if recovery_at > Local::now().naive_local() => {
            // Quote the provider's own phrasing of the time rather than a
            // reformat of it; the parse above only decides whether it is past.
            let stated = marker_field(&content, "recovery_at: ")
                .unwrap_or_else(|| format_recovery(recovery_at));
            Some(format!("until {stated}"))
        }
        StoredHold::NeedsHuman => {
            let slug = marker_slug(agent, custom_name);
            Some(format!("until cleared with `aid config clear-limit {slug}`"))
        }
        StoredHold::Until(_) | StoredHold::Transient => None,
    }
}

/// The bounded cooldown for the transient class, measured from the write time.
fn within_cooldown_window(path: &std::path::Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed.as_secs() < RATE_LIMIT_WINDOW_SECS)
}

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

fn discovered_group_markers(
    agent: &AgentKind,
    custom_name: Option<&str>,
) -> Vec<(String, PathBuf)> {
    if *agent != AgentKind::OpenCode {
        return Vec::new();
    }
    let prefix = format!("rate-limit-{}--", marker_slug(agent, custom_name));
    let Ok(entries) = fs::read_dir(aid_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let group = name.strip_prefix(&prefix)?.to_string();
            (!group.is_empty()).then_some((group, entry.path()))
        })
        .collect()
}

pub fn is_rate_limited(agent: &AgentKind, custom_name: Option<&str>) -> bool {
    marker_is_active(&marker_path(agent, custom_name), agent)
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

/// The agent a quota message names, when the provider's wording identifies it.
/// Lets a caller mark the right agent even when the failure surfaced somewhere
/// that does not know which CLI produced it.
pub fn quota_signature_agent(message: &str) -> Option<AgentKind> {
    crate::rate_limit_signatures::match_quota_signature(message).map(|(agent, _)| agent)
}

/// Where a quota signal was observed. Generic tokens (429, "rate limit") are
/// evidence on channels the agent does not author; they must never match prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaEvidence {
    /// Structured CLI error events, stderr, HTTP status lines.
    NonAgentChannel,
    /// Assistant-authored text — per-agent templates only.
    AgentProse,
}

pub fn is_rate_limit_error(message: &str) -> bool {
    is_rate_limit_error_with_evidence(message, QuotaEvidence::AgentProse)
}

pub fn is_rate_limit_error_with_evidence(message: &str, evidence: QuotaEvidence) -> bool {
    if crate::rate_limit_signatures::match_quota_signature(message).is_some() {
        return true;
    }
    evidence == QuotaEvidence::NonAgentChannel && generic_quota_signal(message)
}

pub fn is_rate_limit_error_for_agent(message: &str, agent: &AgentKind) -> bool {
    is_rate_limit_error_for_agent_with_evidence(message, agent, QuotaEvidence::NonAgentChannel)
}

pub fn is_rate_limit_error_for_agent_with_evidence(
    message: &str,
    agent: &AgentKind,
    evidence: QuotaEvidence,
) -> bool {
    if crate::rate_limit_signatures::match_quota_signature_for_agent(message, *agent).is_some() {
        return true;
    }
    evidence == QuotaEvidence::NonAgentChannel && generic_quota_signal(message)
}

/// A status token that means "out of quota" whoever the provider is. Admissible
/// only inside an envelope the CLI opened — see `quota_channel::Attributable`.
///
/// `rate_limit` is deliberately absent. No provider writes its refusal in
/// snake_case; that spelling only ever appears in source, in a grep pattern or
/// in a report about this crate, so it could match nothing real and forge
/// plenty.
fn generic_quota_signal(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
        || contains_status_code(&lower, "429")
        || contains_status_code(&lower, "402")
        || lower.contains("too many requests")
}

/// Match an HTTP status code only as a standalone number, not inside larger numbers.
fn contains_status_code(s: &str, code: &str) -> bool {
    let bytes = s.as_bytes();
    let code = code.as_bytes();
    if bytes.len() < code.len() {
        return false;
    }
    for i in 0..=bytes.len().saturating_sub(code.len()) {
        if &bytes[i..i + code.len()] == code {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let after_ok = i + code.len() >= bytes.len() || !bytes[i + code.len()].is_ascii_digit();
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

/// The provider's refusal in `raw`, or `None` — the one way captured bytes
/// become a rate-limit marker.
///
/// `raw` is first split by `quota_channel` into what the CLI said and what the
/// model said, and only the CLI's part is matched. Callers name the channel they
/// read from; nothing else about the bytes is consulted, and in particular no
/// caller may hand this an adapter's rendered event detail, an assistant
/// message, or a tool result. Those are not channels — see `quota_channel`.
///
/// The evidence rule follows the split rather than the call site: a string the
/// CLI put inside a diagnostic envelope may carry a bare status token, because
/// only the CLI could have put it there. A line with no envelope around it must
/// match that agent's own anchored signature, because on a PTY transport it may
/// be the model's rendered answer.
pub(crate) fn refusal_on_channel(
    raw: &str,
    agent: AgentKind,
    channel: crate::quota_channel::Channel,
) -> Option<String> {
    let kept = crate::quota_channel::provider_attributable(raw, agent, channel);
    if let Some(refusal) = crate::agent::stream_completion::quota_line(&kept.all(), agent) {
        return Some(refusal);
    }
    let generic = kept.cli_diagnostic.lines().find(|line| generic_quota_signal(line))?;
    let refusal: String = generic.chars().take(240).collect();
    let refusal = refusal.trim();
    (!refusal.is_empty()).then(|| refusal.to_string())
}

fn parse_recovery_time(message: &str) -> Option<String> {
    let prefix = "try again at ";
    if let Some(start) = message.find(prefix) {
        let start = start + prefix.len();
        let remainder = &message[start..];
        let end = remainder.find('.').unwrap_or(remainder.len());
        Some(remainder[..end].trim().to_string())
    } else {
        parse_iso_recovery_time(message)
    }
}

fn parse_iso_recovery_time(message: &str) -> Option<String> {
    for (index, _) in message.match_indices("20") {
        let candidate: String = message[index..]
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | ':' | '.' | '+'))
            .collect();
        let candidate = candidate.trim_end_matches(['.', ',', ';', ')', ']', '}']);
        let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(candidate) else {
            continue;
        };
        let local = parsed.with_timezone(&Local).naive_local();
        return Some(format_recovery(local));
    }
    None
}

fn parse_recovery_datetime(s: &str) -> Option<NaiveDateTime> {
    let mut parts: Vec<String> = s.split(' ').map(|part| part.to_string()).collect();
    if parts.len() < 2 {
        return None;
    }

    let day_token = &parts[1];
    let day_without_comma = day_token.strip_suffix(',').unwrap_or(day_token);
    let day_without_suffix = day_without_comma
        .strip_suffix("st")
        .or_else(|| day_without_comma.strip_suffix("nd"))
        .or_else(|| day_without_comma.strip_suffix("rd"))
        .or_else(|| day_without_comma.strip_suffix("th"))
        .unwrap_or(day_without_comma);
    let day_number: u32 = day_without_suffix.parse().ok()?;
    let day_with_comma = if day_token.ends_with(',') { "," } else { "" };
    parts[1] = format!("{:02}{}", day_number, day_with_comma);

    let cleaned = parts.join(" ");
    NaiveDateTime::parse_from_str(&cleaned, "%b %d, %Y %I:%M %p").ok()
}

#[cfg(test)]
pub(crate) fn test_future_recovery_time() -> String {
    let at = Local::now().naive_local() + chrono::Duration::days(1);
    format!(
        "{} {}{}, {} {}:{:02} {}",
        at.format("%b"),
        at.day(),
        test_ordinal_suffix(at.day()),
        at.year(),
        at.hour12().1,
        at.minute(),
        at.format("%p")
    )
}

#[cfg(test)]
fn test_ordinal_suffix(day: u32) -> &'static str {
    match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}

#[derive(Debug, PartialEq)]
pub struct RateLimitInfo {
    pub recovery_at: Option<String>,
    pub message: Option<String>,
    /// True when only `aid config clear-limit` ends this hold. Distinguishes a
    /// spent balance from a transient refusal, which also has no recovery time
    /// but expires on its own.
    pub needs_human: bool,
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
        .map(|(group, _)| ((*group).to_string(), group_marker_path(agent, custom_name, group)))
        .collect();
    groups.extend(discovered_group_markers(agent, custom_name));
    groups
        .into_iter()
        .filter_map(|(group, path)| {
            if !marker_is_active(&path, agent) {
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
    if let Some(at) = info.recovery_at.as_deref() {
        format!("resets {at}")
    } else if info.needs_human {
        // Named by the same function that decides the marker file, so the
        // command we print can never clear a different agent than the one held:
        // `AgentKind::Custom.as_str()` is the constant "custom", so a held
        // `glm5` used to be reported as cleared by `clear-limit custom`.
        format!(
            "held until cleared with `aid config clear-limit {}`",
            marker_slug(agent, custom_name)
        )
    } else {
        "cooling down".to_string()
    }
}

#[cfg(test)]
#[path = "rate_limit_credibility_tests.rs"]
mod credibility_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    /// The exact string codex produced on 2026-08-05. If this fails to parse,
    /// `is_rate_limited` falls back to a 300-second mtime window and a six-day
    /// outage reads as available again after five minutes.
    #[test]
    fn codex_recovery_timestamp_parses() {
        let stated = test_future_recovery_time();
        let message = format!(
            "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
             to purchase more credits or try again at {stated}."
        );
        let extracted = parse_recovery_time(&message).expect("recovery phrase must be extracted");
        assert_eq!(extracted, stated);
        let parsed = parse_recovery_datetime(&extracted).expect("recovery timestamp must parse");
        assert!(parsed > Local::now().naive_local(), "parsed {parsed} must be in the future");
    }

    #[test]
    fn test_is_rate_limit_error() {
        assert!(is_rate_limit_error(
            "You have hit your usage limit. try again at Mar 19th, 2026 2:27 PM."
        ));
        assert!(is_rate_limit_error(
            "Quota exhausted: Your token-plan 5-hour quota has been exhausted."
        ));
        assert!(is_rate_limit_error("APIError: Insufficient balance. Manage your billing here"));
        assert!(is_rate_limit_error("402 payment required: reload your tokens"));
        assert!(is_rate_limit_error(
            "IneligibleTierError: This client is no longer supported for Gemini Code Assist for individuals; migrate to Antigravity"
        ));
        assert!(!is_rate_limit_error("network timeout"));
        assert!(!is_rate_limit_error("connection refused"));
        assert!(!is_rate_limit_error("payment required"));
        assert!(!is_rate_limit_error(
            "503 No accounts with a plan supporting gpt-4.1-nano"
        ));
        assert!(!is_rate_limit_error(
            "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)"
        ));
        assert!(!is_rate_limit_error("invoice 1402 created"));
    }

    #[test]
    fn prose_mentions_rate_limit_is_not_quota_failure() {
        assert!(!is_rate_limit_error(
            "rate_limit_kind now returns AgentKind::Custom for custom agents"
        ));
        assert!(!is_rate_limit_error(
            "The RPC provider throttles us; we saw a 429 and burned Alchemy credits"
        ));
        assert!(!is_rate_limit_error(
            "We must respect the rate limit on the Base sequencer feed"
        ));
        assert!(!is_rate_limit_error("The parser handles nested arrays correctly"));
    }

    #[test]
    fn generic_quota_signals_apply_on_non_agent_channels() {
        assert!(is_rate_limit_error_with_evidence(
            "rate limit exceeded",
            QuotaEvidence::NonAgentChannel
        ));
        assert!(is_rate_limit_error_with_evidence(
            "HTTP 429 Too Many Requests",
            QuotaEvidence::NonAgentChannel
        ));
        assert!(is_rate_limit_error_for_agent_with_evidence(
            "rate limit exceeded",
            &AgentKind::Claude,
            QuotaEvidence::NonAgentChannel
        ));
        assert!(is_rate_limit_error_for_agent_with_evidence(
            "429 Too Many Requests",
            &AgentKind::Grok,
            QuotaEvidence::NonAgentChannel
        ));
        assert!(!is_rate_limit_error_with_evidence(
            "We must respect the rate limit on the Base sequencer feed",
            QuotaEvidence::AgentProse
        ));
    }

    use crate::quota_channel::Channel;

    #[test]
    fn a_generic_status_token_is_read_inside_an_envelope_the_cli_opened() {
        assert_eq!(
            refusal_on_channel(
                r#"{"type":"error","error":{"message":"429 rate limit exceeded"}}"#,
                AgentKind::Cursor,
                Channel::CliStream,
            ),
            Some("429 rate limit exceeded".to_string())
        );
    }

    #[test]
    fn a_generic_status_token_outside_an_envelope_is_not_a_refusal() {
        // Same token, same channel, no envelope around it: this is the shape a
        // PTY renders the model's own answer in.
        assert_eq!(
            refusal_on_channel("429 rate limit exceeded", AgentKind::Cursor, Channel::CliStream),
            None
        );
    }

    #[test]
    fn stderr_carries_droids_402_body() {
        let line = r#"402 {"detail":"You've reached your 5-hour standard usage limit (resets in 1h 48min).","status":402}"#;
        assert!(
            refusal_on_channel(line, AgentKind::Droid, Channel::CliStderr)
                .is_some_and(|refusal| refusal.contains("standard usage limit")),
            "the refusal droid actually wrote on 2026-08-07 must stay detectable"
        );
    }

    #[test]
    fn an_agents_own_words_about_a_provider_are_not_a_refusal() {
        for line in [
            "The RPC provider throttles us; we saw a 429 during the run.",
            "completed: grep clear_rate_limit_if_stale|marker_path",
            "YOLO mode is enabled",
        ] {
            assert_eq!(
                refusal_on_channel(line, AgentKind::Cursor, Channel::CliStream),
                None,
                "{line:?} is the model or aid talking"
            );
        }
    }

    #[test]
    fn an_init_event_mentioning_rate_limits_is_not_a_refusal() {
        assert_eq!(
            refusal_on_channel(
                r#"{"type":"system","subtype":"init","message":"rate limit enabled"}"#,
                AgentKind::Codex,
                Channel::CliStream,
            ),
            None
        );
    }

    #[test]
    fn codexs_usage_limit_envelope_yields_its_own_sentence() {
        let message = "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
                         to purchase more credits or try again at Aug 11th, 2026 2:23 PM.";
        assert_eq!(
            refusal_on_channel(
                &format!(r#"{{"type":"error","message":"{message}"}}"#),
                AgentKind::Codex,
                Channel::CliStream,
            ),
            Some(message.to_string()),
        );
    }

    /// A signature belongs to one provider. cursor's needle in copilot's stream
    /// is a report about cursor, not copilot refusing.
    #[test]
    fn a_refusal_is_only_read_for_the_agent_that_owns_the_signature() {
        let envelope =
            r#"{"type":"error","message":"You're out of usage. Switch to Auto."}"#;
        assert!(refusal_on_channel(envelope, AgentKind::Cursor, Channel::CliStream).is_some());
        assert_eq!(
            refusal_on_channel(envelope, AgentKind::Copilot, Channel::CliStream),
            None
        );
    }

    #[test]
    fn test_mark_and_check_rate_limited() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");
        assert!(is_rate_limited(&AgentKind::Codex, None));

        let _ = std::fs::remove_file(marker_path(&AgentKind::Codex, None));
        assert!(!is_rate_limited(&AgentKind::Codex, None));
    }

    #[test]
    fn test_is_rate_limited_returns_false_for_fresh_agents() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-fresh");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        assert!(!is_rate_limited(&AgentKind::Codex, None));
    }

    #[test]
    fn test_rate_limited_agents_returns_empty_initially() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-empty");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        assert!(rate_limited_agents().is_empty());
    }

    #[test]
    fn test_parse_recovery_time() {
        assert_eq!(
            parse_recovery_time("You have hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Mar 19th, 2026 2:27 PM."),
            Some("Mar 19th, 2026 2:27 PM".to_string())
        );
        assert_eq!(parse_recovery_time("no recovery time here"), None);
        assert_eq!(
            parse_recovery_time("try again at tomorrow morning."),
            Some("tomorrow morning".to_string())
        );
    }

    #[test]
    fn test_parse_recovery_datetime() {
        let parsed = parse_recovery_datetime("Mar 19th, 2026 2:27 PM").unwrap();
        let expected =
            NaiveDateTime::parse_from_str("Mar 19, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(parsed, expected);

        let first = parse_recovery_datetime("Mar 1st, 2026 2:27 PM").unwrap();
        let expected_first =
            NaiveDateTime::parse_from_str("Mar 01, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(first, expected_first);

        let second = parse_recovery_datetime("Mar 2nd, 2026 2:27 PM").unwrap();
        let expected_second =
            NaiveDateTime::parse_from_str("Mar 02, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(second, expected_second);

        let third = parse_recovery_datetime("Mar 3rd, 2026 2:27 PM").unwrap();
        let expected_third =
            NaiveDateTime::parse_from_str("Mar 03, 2026 2:27 PM", "%b %d, %Y %I:%M %p").unwrap();
        assert_eq!(third, expected_third);

        assert!(parse_recovery_datetime("not a date").is_none());
    }

    #[test]
    fn test_is_rate_limited_expired() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-expired");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        let past = Local::now().naive_local() - chrono::Duration::minutes(5);
        let recovery_at = past.format("%b %d, %Y %I:%M %p").to_string();
        let content = format!("recovery_at: {}\nmessage: test\n", recovery_at);
        let path = marker_path(&AgentKind::Codex, None);
        let _ = std::fs::write(&path, content);

        assert!(!is_rate_limited(&AgentKind::Codex, None));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_get_rate_limit_info() {
        let temp_dir = std::env::temp_dir().join("aid-rate-limit-test-info");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok();

        // Test with recovery time
        mark_rate_limited(&AgentKind::Codex, None, "You have hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Mar 19th, 2026 2:27 PM.");
        let info = get_rate_limit_info(&AgentKind::Codex, None).unwrap();
        assert_eq!(info.recovery_at, Some("Mar 19th, 2026 2:27 PM".to_string()));
        assert!(info
            .message
            .unwrap()
            .contains("You have hit your usage limit"));

        // Test without recovery time
        mark_rate_limited(&AgentKind::Gemini, None, "rate limit exceeded");
        let info = get_rate_limit_info(&AgentKind::Gemini, None).unwrap();
        assert_eq!(info.recovery_at, None);
        assert_eq!(info.message, Some("rate limit exceeded".to_string()));

        mark_rate_limited(&AgentKind::Qwen, None, "rate limit exceeded");
        let info = get_rate_limit_info(&AgentKind::Qwen, None).unwrap();
        assert_eq!(info.recovery_at, None);
        assert_eq!(info.message, Some("rate limit exceeded".to_string()));

        // Test non-existent file
        assert!(get_rate_limit_info(&AgentKind::Cursor, None).is_none());

        let _ = std::fs::remove_file(marker_path(&AgentKind::Codex, None));
        let _ = std::fs::remove_file(marker_path(&AgentKind::Gemini, None));
        let _ = std::fs::remove_file(marker_path(&AgentKind::Qwen, None));
    }
}

#[cfg(test)]
mod stale_clear_tests {
    use super::*;
    use crate::types::AgentKind;

    /// A marker this run wrote must outlive this run's success. Clearing it is
    /// how an outage recorded by `record_quota_exhaustion` was handed straight
    /// back to routing by `handle_done_postprocess`.
    #[test]
    fn a_marker_written_during_the_run_is_not_cleared_by_success() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        clear_rate_limit(&AgentKind::Qwen, None);

        let task_start = Local::now() - chrono::Duration::minutes(5);
        mark_rate_limited(&AgentKind::Qwen, None, "Your token-plan 5-hour quota has been exhausted.");

        assert!(!clear_rate_limit_if_stale(&AgentKind::Qwen, None, task_start));
        assert!(is_rate_limited(&AgentKind::Qwen, None));
    }

    /// A marker left by an earlier run is stale and a fresh success clears it,
    /// exactly as before this change.
    #[test]
    fn a_marker_from_an_earlier_run_is_still_cleared() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        clear_rate_limit(&AgentKind::Qwen, None);

        mark_rate_limited(&AgentKind::Qwen, None, "Your token-plan 5-hour quota has been exhausted.");
        let task_start = Local::now() + chrono::Duration::minutes(5);

        assert!(clear_rate_limit_if_stale(&AgentKind::Qwen, None, task_start));
        assert!(!is_rate_limited(&AgentKind::Qwen, None));
    }

    #[test]
    fn clear_group_rate_limit_if_stale_clears_only_matching_group() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let agent = AgentKind::Antigravity;
        clear_all_rate_limits_for_agent(&agent, None);

        mark_rate_limited(&agent, None, "Agent rate limit");
        mark_group_rate_limited(&agent, None, "gemini", "Gemini quota exhausted");
        mark_group_rate_limited(&agent, None, "claude", "Claude quota exhausted");

        let task_start = Local::now() + chrono::Duration::minutes(5);

        let cleared = clear_rate_limit_for_model_if_stale(
            &agent,
            None,
            Some("gemini-3.6-flash-high"),
            task_start,
        );
        assert!(cleared, "gemini group marker should be cleared on success");

        assert!(!is_rate_limited(&agent, None), "agent-level marker must be cleared on model success");
        assert!(!is_group_rate_limited(&agent, None, "gemini"), "gemini group must no longer be limited");
        assert!(is_group_rate_limited(&agent, None, "claude"), "claude group must remain limited");
    }

    #[test]
    fn clear_rate_limit_does_not_clear_group_markers() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let agent = AgentKind::Antigravity;
        clear_all_rate_limits_for_agent(&agent, None);

        mark_rate_limited(&agent, None, "Agent level limit");
        mark_group_rate_limited(&agent, None, "gemini", "Gemini quota exhausted");

        assert!(clear_rate_limit(&agent, None));
        assert!(!is_rate_limited(&agent, None), "agent-level marker must be removed");
        assert!(is_group_rate_limited(&agent, None, "gemini"), "group marker must NOT be removed by clear_rate_limit");
    }

    #[test]
    fn clear_all_rate_limits_clears_agent_and_all_groups() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let agent = AgentKind::Antigravity;
        clear_all_rate_limits_for_agent(&agent, None);

        mark_rate_limited(&agent, None, "Agent limit");
        mark_group_rate_limited(&agent, None, "gemini", "Gemini limit");
        mark_group_rate_limited(&agent, None, "claude", "Claude limit");

        assert!(clear_all_rate_limits_for_agent(&agent, None));
        assert!(!is_rate_limited(&agent, None));
        assert!(!is_group_rate_limited(&agent, None, "gemini"));
        assert!(!is_group_rate_limited(&agent, None, "claude"));
    }
}

#[cfg(test)]
#[path = "rate_limit_hold_tests.rs"]
mod hold_tests;

#[cfg(test)]
#[path = "rate_limit_custom_tests.rs"]
mod custom_tests;

#[cfg(test)]
mod home_guard_tests {
    use super::*;
    use crate::paths::{self, AidHomeGuard};

    #[test]
    fn marker_path_writes_under_isolated_home() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(paths::aid_dir()).unwrap();

        mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");
        let marker = paths::aid_dir().join("rate-limit-codex");
        assert!(marker.exists());
        assert!(marker.starts_with(temp.path()));
    }

    #[test]
    fn marker_path_refuses_real_home_without_guard() {
        let resolved = paths::aid_dir();
        let home = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let real = home.join(".aid");
        if resolved != real {
            return;
        }
        let err = std::panic::catch_unwind(|| {
            let _ = marker_path(&AgentKind::Codex, None);
        });
        assert!(
            err.is_err(),
            "marker_path must refuse real ~/.aid without AidHomeGuard"
        );
    }
}
