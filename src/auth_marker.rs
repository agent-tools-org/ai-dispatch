// Observed-bad auth evidence per agent: set from a recognised not-signed-in run failure.
// Exports: AuthStatus, auth_status, record_from_run, record_failure_at, clear_on_success.
// Deps: paths, quota_channel (provider-attributable lines), chrono, serde.

use std::path::PathBuf;

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::quota_channel::{provider_attributable, Channel};
use crate::types::AgentKind;

/// An observed-bad verdict is trusted for this long, then reverts to unknown.
pub(crate) const AUTH_FAILED_TTL_SECS: i64 = 3600;

/// CLI wording that means "this account is not signed in". Matched only on
/// provider-attributable lines, so a model quoting this table cannot set it.
const AUTH_SIGNATURES: &[(AgentKind, &str)] = &[
    // `{"type":"error","message":"Not signed in. To authenticate ... grok login"}`
    (AgentKind::Grok, "not signed in"),
    // "Invalid API key · Please run /login" / "Not logged in · Please run /login"
    (AgentKind::Claude, "please run /login"),
    (AgentKind::Claude, "not logged in"),
    // "Error: Your credentials are invalid. Please log in again with `oz login`."
    (AgentKind::Oz, "credentials are invalid"),
];

/// Auth evidence for one route. `state` is `failed` or `unknown`; aid never
/// reports `ok` because no probe observes a successful sign-in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Default for AuthStatus {
    fn default() -> Self {
        Self { state: "unknown".to_string(), observed_at: None, message: None }
    }
}

impl AuthStatus {
    pub fn failed(&self) -> bool {
        self.state == "failed"
    }
}

fn marker_path(agent: AgentKind) -> PathBuf {
    crate::paths::aid_dir().join(format!("auth-failed-{}", agent.as_str()))
}

pub(crate) fn auth_status(agent: AgentKind, custom_name: Option<&str>) -> AuthStatus {
    auth_status_at(agent, custom_name, Local::now())
}

fn auth_status_at(agent: AgentKind, custom_name: Option<&str>, now: DateTime<Local>) -> AuthStatus {
    if agent == AgentKind::Custom || custom_name.is_some() {
        return AuthStatus::default();
    }
    let Ok(content) = std::fs::read_to_string(marker_path(agent)) else {
        return AuthStatus::default();
    };
    let field = |key: &str| content.lines().find_map(|line| line.strip_prefix(key)).map(str::to_string);
    let Some(observed) = field("observed_at: ")
        .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| at.with_timezone(&Local))
    else {
        return AuthStatus::default();
    };
    if now - observed >= Duration::seconds(AUTH_FAILED_TTL_SECS) {
        return AuthStatus::default();
    }
    AuthStatus {
        state: "failed".to_string(),
        observed_at: Some(observed.to_rfc3339()),
        message: field("message: "),
    }
}

/// The CLI's own not-signed-in line in this run output, if any.
pub(crate) fn auth_failure_line(raw: &str, agent: AgentKind, channel: Channel) -> Option<String> {
    let needles: Vec<&str> = AUTH_SIGNATURES
        .iter()
        .filter(|(kind, _)| *kind == agent)
        .map(|(_, needle)| *needle)
        .collect();
    if needles.is_empty() {
        return None;
    }
    let kept = provider_attributable(raw, agent, channel).all();
    kept.lines()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            needles.iter().any(|needle| lower.contains(needle))
        })
        .map(|line| line.trim().chars().take(240).collect())
}

/// Read a failed run's stderr and stream log; write the marker on a match.
pub(crate) fn record_from_run(task_id: &str, agent: AgentKind) -> bool {
    let sources = [
        (crate::paths::stderr_path(task_id), Channel::CliStderr),
        (crate::paths::log_path(task_id), Channel::CliStream),
    ];
    let line = sources.iter().find_map(|(path, channel)| {
        let raw = std::fs::read_to_string(path).ok()?;
        auth_failure_line(&raw, agent, *channel)
    });
    match line {
        Some(line) => {
            record_failure_at(agent, &line, Local::now());
            true
        }
        None => false,
    }
}

pub(crate) fn record_failure_at(agent: AgentKind, message: &str, at: DateTime<Local>) {
    let first = message.lines().next().unwrap_or_default();
    let body = format!("observed_at: {}\nmessage: {first}\n", at.to_rfc3339());
    let path = marker_path(agent);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(err) = std::fs::write(&path, body) {
        aid_warn!("[aid] Failed to record auth failure for {}: {err}", agent.as_str());
    }
}

/// A successful run is evidence the route is signed in; drop any failed verdict.
pub(crate) fn clear_on_success(agent: AgentKind) {
    let _ = std::fs::remove_file(marker_path(agent));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;

    const GROK_ERROR: &str = r#"{"type":"error","message":"Not signed in. To authenticate without a browser, run:\n  grok login --device-code"}"#;

    #[test]
    fn no_evidence_is_unknown() {
        let home = tempfile::tempdir().expect("home");
        let _guard = AidHomeGuard::set(home.path());
        let status = auth_status(AgentKind::Grok, None);
        assert_eq!(status.state, "unknown");
        assert_eq!(status.observed_at, None);
    }

    #[test]
    fn not_signed_in_run_sets_failed_with_observed_time() {
        let home = tempfile::tempdir().expect("home");
        let _guard = AidHomeGuard::set(home.path());
        std::fs::create_dir_all(crate::paths::logs_dir()).expect("logs");
        std::fs::write(crate::paths::log_path("t-auth"), format!("{GROK_ERROR}\n")).expect("log");
        assert!(record_from_run("t-auth", AgentKind::Grok));
        let status = auth_status(AgentKind::Grok, None);
        assert_eq!(status.state, "failed");
        assert!(status.observed_at.is_some());
        assert!(status.message.as_deref().is_some_and(|m| m.starts_with("Not signed in")));
        assert_eq!(auth_status(AgentKind::Codex, None).state, "unknown");
    }

    #[test]
    fn failed_verdict_expires_after_ttl() {
        let home = tempfile::tempdir().expect("home");
        let _guard = AidHomeGuard::set(home.path());
        let at = Local::now() - Duration::seconds(AUTH_FAILED_TTL_SECS + 5);
        record_failure_at(AgentKind::Grok, "Not signed in.", at);
        assert_eq!(auth_status(AgentKind::Grok, None).state, "unknown");
        let fresh = Local::now() - Duration::seconds(AUTH_FAILED_TTL_SECS - 60);
        record_failure_at(AgentKind::Grok, "Not signed in.", fresh);
        assert_eq!(auth_status(AgentKind::Grok, None).state, "failed");
        let later = fresh + Duration::seconds(AUTH_FAILED_TTL_SECS);
        assert_eq!(auth_status_at(AgentKind::Grok, None, later).state, "unknown");
    }

    #[test]
    fn successful_run_clears_failed_verdict() {
        let home = tempfile::tempdir().expect("home");
        let _guard = AidHomeGuard::set(home.path());
        record_failure_at(AgentKind::Claude, "Invalid API key · Please run /login", Local::now());
        assert!(auth_status(AgentKind::Claude, None).failed());
        clear_on_success(AgentKind::Claude);
        assert_eq!(auth_status(AgentKind::Claude, None).state, "unknown");
    }

    #[test]
    fn unmatched_or_missing_output_never_marks_failed() {
        let home = tempfile::tempdir().expect("home");
        let _guard = AidHomeGuard::set(home.path());
        assert!(!record_from_run("t-missing", AgentKind::Grok));
        let quoted = r#"{"type":"assistant","message":{"content":[{"text":"Not signed in"}]}}"#;
        assert_eq!(auth_failure_line(quoted, AgentKind::Grok, Channel::CliStream), None);
        assert_eq!(auth_failure_line("Not signed in", AgentKind::Codex, Channel::CliStderr), None);
        assert_eq!(auth_status(AgentKind::Grok, None).state, "unknown");
    }
}
