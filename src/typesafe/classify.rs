// One classify call shared by `aid classify` and the MCP `classify` tool: validate, screen, send, check.
// Exports: ClassifyRequest, ClassifyState, ClassifyError, ErrorKind, classify, DEFAULT_MODEL.
// Deps: typesafe::{question, screen, answer, secret}; curl via secret::post_json.

use super::answer::validate_response;
use super::question::{Declared, validate_questions};
use super::screen::{screen_json, screen_text};
use super::secret;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_MODEL: &str = "jev-1.13.0";
pub(crate) const DEFAULT_TIMEOUT_SECS: u64 = 20;
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const RETRY_BACKOFF: Duration = Duration::from_secs(1);
pub(crate) const KEY_SETUP: &str = "security add-generic-password -a \"$USER\" -s typesafe-api-key -U -w";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    NoKey,
    Api,
    Invalid,
    Refused,
}

/// A one-line message plus the exit class; messages never carry the key or response content.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ClassifyError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ClassifyError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub(crate) fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::NoKey => 2,
            ErrorKind::Api => 3,
            ErrorKind::Invalid => 4,
            ErrorKind::Refused => 5,
        }
    }
}

impl std::fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ClassifyState {
    Text(String),
    Json(Value),
}

#[derive(Clone, Debug)]
pub(crate) struct ClassifyRequest {
    pub state: ClassifyState,
    pub questions: Map<String, Value>,
    pub model: String,
    pub timeout_secs: u64,
    pub allow_secret_like: bool,
}

/// Validates and screens locally first; the keychain is read only for a request that may be sent.
pub(crate) fn classify(request: &ClassifyRequest) -> Result<Value, ClassifyError> {
    let declared = prepare(request)?;
    let key = resolve_key().ok_or_else(|| {
        ClassifyError::new(
            ErrorKind::NoKey,
            format!("no TypeSafe key in the login keychain; run once in a real terminal: {KEY_SETUP}"),
        )
    })?;
    let state = match &request.state {
        ClassifyState::Text(text) => json!(text),
        ClassifyState::Json(value) => value.clone(),
    };
    let body = json!({ "state": state, "model": request.model, "questions": request.questions }).to_string();
    let started = Instant::now();
    let post = || secret::post_json(ENDPOINT, &key, &body, request.timeout_secs);
    let (status, text) = send_with_retry(post, std::thread::sleep)?;
    finish(status, &text, &declared, started.elapsed())
}

fn prepare(request: &ClassifyRequest) -> Result<BTreeMap<String, Declared>, ClassifyError> {
    let invalid = |message: String| ClassifyError::new(ErrorKind::Invalid, message);
    if !is_model_name(&request.model) {
        return Err(invalid("model must be jev-<version> or a jev- alias".into()));
    }
    if !(1..=300).contains(&request.timeout_secs) {
        return Err(invalid("timeout must be 1-300 seconds".into()));
    }
    let declared = validate_questions(&request.questions).map_err(invalid)?;
    let screened = match &request.state {
        ClassifyState::Text(text) => screen_text(text, request.allow_secret_like),
        ClassifyState::Json(value) => screen_json(value, request.allow_secret_like),
    };
    screened.map_err(|refusal| match refusal {
        super::screen::StateRefusal::Empty => invalid(refusal.message()),
        _ => ClassifyError::new(ErrorKind::Refused, refusal.message()),
    })?;
    Ok(declared)
}

fn is_model_name(model: &str) -> bool {
    model.len() <= 64
        && model.strip_prefix("jev-").is_some_and(|rest| {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
}

/// Debug builds accept a switch that supplies a fixed fake key (or none) so tests never need the
/// keychain; the switch cannot carry a real key and does not exist in release builds.
fn resolve_key() -> Option<String> {
    #[cfg(debug_assertions)]
    match std::env::var("AID_TYPESAFE_TEST_KEY").as_deref() {
        Ok("fake") => return Some("ts_fake_key_for_tests".to_string()),
        Ok("absent") => return None,
        _ => {}
    }
    secret::api_key()
}

/// Sends once, and once more after a short backoff when TypeSafe answers 429 or 529.
fn send_with_retry(
    mut post: impl FnMut() -> anyhow::Result<(u16, String)>,
    sleep: impl Fn(Duration),
) -> Result<(u16, String), ClassifyError> {
    let network = |_| ClassifyError::new(ErrorKind::Api, "TypeSafe request failed: no response (network error, timeout, or curl missing)");
    let first = post().map_err(network)?;
    if !matches!(first.0, 429 | 529) {
        return Ok(first);
    }
    sleep(RETRY_BACKOFF);
    post().map_err(network)
}

fn finish(
    status: u16,
    body: &str,
    declared: &BTreeMap<String, Declared>,
    latency: Duration,
) -> Result<Value, ClassifyError> {
    if !(200..300).contains(&status) {
        let detail = api_error_type(body).map(|kind| format!(" ({kind})")).unwrap_or_default();
        return Err(ClassifyError::new(ErrorKind::Api, format!("TypeSafe API error: HTTP {status}{detail}")));
    }
    let mut out = validate_response(body, declared)
        .map_err(|reason| ClassifyError::new(ErrorKind::Api, format!("TypeSafe returned an invalid response: {reason}")))?;
    out["latency_ms"] = json!(u64::try_from(latency.as_millis()).unwrap_or(u64::MAX));
    Ok(out)
}

/// The API's error type label, only when it is a short plain identifier.
fn api_error_type(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let kind = value
        .pointer("/error/type")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)?;
    let plain = !kind.is_empty() && kind.len() <= 64
        && kind.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    plain.then(|| kind.to_string())
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
