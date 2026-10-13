// Observed-bad auth evidence per agent: set from a recognised not-signed-in run failure.
// Exports: AuthStatus, auth_status, record_from_run, record_failure_at, clear_on_success.
// Deps: paths, quota_channel (provider-attributable lines), chrono, serde.

use std::path::PathBuf;

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::quota_channel::{provider_attributable, Channel};
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};

/// Auth observations without credential stamps expire after this interval.
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
    let field = |key: &str| {
        content
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .map(str::to_string)
    };
    let Some(observed) = field("observed_at: ")
        .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| at.with_timezone(&Local))
    else {
        return AuthStatus::default();
    };
    let expired = if agent == AgentKind::OpenCode {
        field("credentials: ").as_deref() != Some(opencode_credential_stamp().as_str())
    } else {
        now - observed >= Duration::seconds(AUTH_FAILED_TTL_SECS)
    };
    if expired {
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
    if agent == AgentKind::OpenCode {
        if channel != Channel::CliStream {
            return None;
        }
        return raw.lines().find_map(|line| {
            let cleaned = crate::watcher::strip_terminal_escapes(line);
            let value: serde_json::Value = serde_json::from_str(cleaned.trim()).ok()?;
            if !crate::quota_channel::is_error_envelope(value.as_object()?) {
                return None;
            }
            let data = &value["error"]["data"];
            (value["type"] == "error"
                && value["error"]["name"] == "APIError"
                && data["statusCode"] == 401
                && !data["message"].as_str().is_some_and(|text| {
                    crate::rate_limit_signatures::match_quota_signature_for_agent(text, agent).is_some()
                }))
            .then(|| "OpenCode APIError statusCode 401".to_string())
        });
    }
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

/// Read failed-run evidence; OpenCode requires decoded events and a failed process exit.
pub(crate) fn record_from_run(task_id: &str, agent: AgentKind, store: &Store) -> bool {
    let line = if agent == AgentKind::OpenCode {
        let Ok(Some(task)) = store.get_task(task_id) else {
            return false;
        };
        if task.status != TaskStatus::Failed || !task.exit_code.is_some_and(|code| code != 0) {
            return false;
        }
        store.get_events(task_id).ok().and_then(|events| {
            events
                .iter()
                .rev()
                .filter(|event| event.timestamp >= task.created_at)
                .filter_map(|event| event.metadata.as_ref())
                .find(|metadata| {
                    metadata.get("opencode_auth_failure").is_some()
                        || metadata["assistant_output"] == true
                })
                .and_then(|metadata| {
                    metadata["opencode_auth_failure"]
                        .as_str()
                        .map(str::to_owned)
                })
        })
    } else {
        let sources = [
            (crate::paths::stderr_path(task_id), Channel::CliStderr),
            (crate::paths::log_path(task_id), Channel::CliStream),
        ];
        sources.iter().find_map(|(path, channel)| {
            let raw = if *channel == Channel::CliStream {
                crate::quota_channel::read_run_stream(task_id)?
            } else {
                std::fs::read_to_string(path).ok()?
            };
            auth_failure_line(&raw, agent, *channel)
        })
    };
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
    let credentials = if agent == AgentKind::OpenCode {
        format!("credentials: {}\n", opencode_credential_stamp())
    } else {
        String::new()
    };
    let body = format!(
        "observed_at: {}\nmessage: {first}\n{credentials}",
        at.to_rfc3339()
    );
    let path = marker_path(agent);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(err) = std::fs::write(&path, body) {
        aid_warn!(
            "[aid] Failed to record auth failure for {}: {err}",
            agent.as_str()
        );
    }
}

/// A successful run is evidence the route is signed in; drop any failed verdict.
pub(crate) fn clear_on_success(agent: AgentKind) {
    let _ = std::fs::remove_file(marker_path(agent));
}

fn credential_stamp(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| {
            let metadata = std::fs::metadata(path)
                .ok()
                .map(|meta| (meta.len(), meta.modified().ok()));
            format!("{path:?}:{metadata:?}")
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn opencode_credential_stamp() -> String {
    #[cfg(test)]
    return credential_stamp(&[crate::paths::aid_dir().join("opencode-auth.json")]);
    #[cfg(not(test))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("opencode");
        let mut paths = vec![
            data.join("opencode/auth.json"),
            config.join("opencode.json"),
            config.join("opencode.jsonc"),
        ];
        paths.extend(std::env::var_os("OPENCODE_CONFIG").map(PathBuf::from));
        if let Some(dir) = std::env::var_os("OPENCODE_CONFIG_DIR") {
            paths.extend(["opencode.json", "opencode.jsonc"].map(|name| PathBuf::from(&dir).join(name)));
        }
        credential_stamp(&paths)
    }
}

#[cfg(test)]
#[path = "auth_marker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "auth_marker_refusal_tests.rs"]
mod refusal_tests;
