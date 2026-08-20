// Local web UI server for `aid web`.
// Exports: serve, api, sse.
// Deps: axum, tower_http, crate::store, crate::web::embed.

pub mod api;
mod api_types;
mod auth;
mod diff;
pub mod embed;
pub mod fleet;
pub mod sse;

#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod data_tests;

use anyhow::Result;
use axum::extract::Path;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::{Method, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::store::Store;

pub async fn serve(store: Arc<Store>, port: u16, host: String, token: Option<String>) -> Result<()> {
    auth::validate_bind_auth(&host, token.as_deref())?;
    let app = build_router(store, port, host.clone(), token.clone());

    let mut addresses = tokio::net::lookup_host((host.as_str(), port)).await?;
    let address = addresses
        .next()
        .ok_or_else(|| anyhow::anyhow!("Unable to resolve web host '{host}'"))?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("[aid] web listening on http://{host}:{port}");
    if let Some(token) = token.as_deref()
        && !auth::is_loopback_host(&host)
    {
        let token_path = auth::persist_token(token)?;
        println!("[aid] client token: {token}  (also at {})", token_path.display());
    }

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn build_router(store: Arc<Store>, port: u16, host: String, token: Option<String>) -> Router {
    let api = Router::new()
        .route("/api/tasks", get(api::list_tasks))
        .route("/api/tasks/{id}", get(api::get_task))
        .route("/api/tasks/{id}/events", get(api::get_task_events))
        .route("/api/tasks/{id}/output", get(api::get_task_output))
        .route("/api/tasks/{id}/stop", post(api::stop_task))
        .route("/api/tasks/{id}/retry", post(api::retry_task))
        .route("/api/tasks/{id}/merge", post(api::merge_task))
        .route("/api/tasks/{id}/diff", get(api::get_task_diff))
        .route("/api/tasks/{id}/result", get(api::get_task_result))
        .route("/api/tasks/{id}/steer", post(api::steer_task))
        .route("/api/tasks/{id}/respond", post(api::respond_task))
        .route("/api/tasks/{id}/accept", post(api::accept_task))
        .route("/api/tasks/{id}/reject", post(api::reject_task))
        .route("/api/usage", get(api::get_usage))
        .route("/api/fleet", get(fleet::get_fleet))
        .route("/api/agents", get(fleet::get_agents))
        .route("/api/events", get(|state| async move { sse::sse_handler(state) }))
        .layer(middleware::from_fn(auth::middleware));
    Router::new()
        .merge(api)
        .route("/", get(index))
        .route("/{*path}", get(serve_static))
        .layer(cors_layer())
        .with_state(store)
        .layer(axum::Extension(fleet::ServerInfo {
            host,
            port,
            started_at: chrono::Utc::now().to_rfc3339(),
        }))
        .layer(axum::Extension(auth::AuthConfig::new(token)))
}

async fn index() -> Response {
    serve_static(Path(String::new())).await
}

async fn serve_static(Path(path): Path<String>) -> Response {
    let asset_path = normalize_asset_path(&path);
    match embed::get_asset(&asset_path).or_else(|| embed::get_asset("index.html")) {
        Some((content_type, body)) => (
            [(CONTENT_TYPE, HeaderValue::from_static(content_type))],
            body,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Missing embedded web assets").into_response(),
    }
}

fn normalize_asset_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return "index.html".to_string();
    }
    trimmed.to_string()
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            is_localhost_origin(origin)
        }))
}

fn is_localhost_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    origin == "http://127.0.0.1"
        || origin == "http://localhost"
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://localhost:")
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header::AUTHORIZATION};
    use axum::response::Response;
    use tower::ServiceExt;

    #[test]
    fn normalize_asset_path_defaults_to_index() {
        assert_eq!(normalize_asset_path(""), "index.html");
        assert_eq!(normalize_asset_path("/app.js"), "app.js");
    }

    #[test]
    fn localhost_origin_filter_accepts_only_local_hosts() {
        assert!(is_localhost_origin(&HeaderValue::from_static("http://127.0.0.1:3000")));
        assert!(is_localhost_origin(&HeaderValue::from_static("http://localhost:5173")));
        assert!(!is_localhost_origin(&HeaderValue::from_static("https://example.com")));
    }

    #[test]
    fn serve_is_exposed() {
        let _ = serve;
    }

    #[tokio::test]
    async fn non_loopback_startup_without_token_is_refused() {
        let store = std::sync::Arc::new(crate::store::Store::open_memory().expect("store"));
        let error = serve(store, 0, "0.0.0.0".to_string(), None)
            .await
            .expect_err("unauthenticated LAN bind must not start");
        assert!(error.to_string().contains("--token"));
    }

    #[tokio::test]
    async fn assembled_router_authenticates_fleet_and_agents() {
        let home = tempfile::tempdir().expect("temporary AID home");
        let _home = crate::paths::AidHomeGuard::set(home.path());
        let store = std::sync::Arc::new(crate::store::Store::open_memory().expect("store"));
        let app = build_router(store, 8080, "127.0.0.1".to_string(), Some("secret".to_string()));
        for uri in ["/api/fleet?window=all", "/api/agents"] {
            assert_eq!(call(&app, uri, None).await.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(call(&app, uri, Some("wrong")).await.status(), StatusCode::UNAUTHORIZED);
            let response = call(&app, uri, Some("secret")).await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.expect("response body");
            serde_json::from_slice::<serde_json::Value>(&body).expect("valid JSON response");
        }
    }

    async fn call(app: &Router, uri: &str, token: Option<&str>) -> Response {
        let mut request = Request::builder().uri(uri);
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        app.clone()
            .oneshot(request.body(Body::empty()).expect("request"))
            .await
            .expect("router response")
    }
}
