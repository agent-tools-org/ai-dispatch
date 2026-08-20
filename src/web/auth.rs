// Bearer authentication and bind safety for the HTTP API surface.
// Exports: AuthConfig, bind validation, and the `/api` auth middleware.
// Deps: axum request/response types and anyhow.

use anyhow::{Result, bail};
use axum::extract::Extension;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const AUTH_FAILURE_LIMIT: usize = 10;
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AuthConfig {
    token: Option<String>,
    failures: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl AuthConfig {
    pub(crate) fn new(token: Option<String>) -> Self {
        Self { token, failures: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn record_failure(&self, peer: &str) -> bool {
        let Ok(mut failures) = self.failures.lock() else {
            return false;
        };
        let cutoff = Instant::now() - AUTH_FAILURE_WINDOW;
        let attempts = failures.entry(peer.to_string()).or_default();
        attempts.retain(|attempt| *attempt >= cutoff);
        attempts.push(Instant::now());
        attempts.len() > AUTH_FAILURE_LIMIT
    }
}

#[derive(Debug, Serialize)]
struct UnauthorizedResponse {
    error: &'static str,
}

pub fn validate_bind_auth(host: &str, token: Option<&str>) -> Result<()> {
    if is_loopback_host(host) {
        return Ok(());
    }
    if token.map(str::trim).is_none_or(str::is_empty) {
        bail!("Non-loopback web binding requires --token");
    }
    Ok(())
}

pub(crate) fn persist_token(token: &str) -> Result<PathBuf> {
    let path = crate::paths::aid_dir().join("web_token");
    std::fs::create_dir_all(crate::paths::aid_dir())?;
    std::fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

pub(crate) async fn middleware(
    Extension(config): Extension<AuthConfig>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if is_authorized(&config, request.headers(), request.uri().query()) {
        return next.run(request).await;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if config.record_failure(&peer) {
        return (StatusCode::TOO_MANY_REQUESTS, Json(UnauthorizedResponse { error: "unauthorized" }))
            .into_response();
    }
    unauthorized()
}

pub(crate) fn is_authorized(config: &AuthConfig, headers: &HeaderMap, query: Option<&str>) -> bool {
    let Some(expected) = config.token.as_deref() else {
        return true;
    };
    bearer_token(headers)
        .or_else(|| query_token(query))
        .is_some_and(|candidate| candidate == expected)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn query_token(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|part| part.strip_prefix("token="))
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(UnauthorizedResponse { error: "unauthorized" })).into_response()
}

pub(crate) fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().map(|address| address.is_loopback()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_loopback_bind_requires_token() {
        let error = validate_bind_auth("0.0.0.0", None).expect_err("LAN bind must require auth");
        assert!(error.to_string().contains("--token"));
    }

    #[test]
    fn wrong_bearer_is_401_and_correct_bearer_is_accepted() {
        let config = AuthConfig::new(Some("secret".to_string()));
        let wrong = HeaderMap::from_iter([(axum::http::header::AUTHORIZATION, "Bearer wrong".parse().expect("header"))]);
        let correct = HeaderMap::from_iter([(axum::http::header::AUTHORIZATION, "Bearer secret".parse().expect("header"))]);
        assert_eq!(authorization_status(&config, &wrong, None), StatusCode::UNAUTHORIZED);
        assert_eq!(unauthorized().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(authorization_status(&config, &correct, None), StatusCode::OK);
    }

    fn authorization_status(config: &AuthConfig, headers: &HeaderMap, query: Option<&str>) -> StatusCode {
        if is_authorized(config, headers, query) { StatusCode::OK } else { StatusCode::UNAUTHORIZED }
    }
}
