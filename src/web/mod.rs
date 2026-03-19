// Local web UI server for `aid web`.
// Exports: serve, api, sse.
// Deps: axum, tower_http, crate::store, crate::web::embed.

pub mod api;
pub mod embed;
pub mod sse;

#[cfg(test)]
mod api_tests;

use anyhow::Result;
use axum::extract::Path;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::store::Store;

pub async fn serve(store: Arc<Store>, port: u16) -> Result<()> {
    let app = Router::new()
        .route("/api/tasks", get(api::list_tasks))
        .route("/api/tasks/{id}", get(api::get_task))
        .route("/api/tasks/{id}/events", get(api::get_task_events))
        .route("/api/tasks/{id}/output", get(api::get_task_output))
        .route("/api/tasks/{id}/stop", post(api::stop_task))
        .route("/api/tasks/{id}/retry", post(api::retry_task))
        .route("/api/tasks/{id}/merge", post(api::merge_task))
        .route("/api/tasks/{id}/diff", get(api::get_task_diff))
        .route("/api/usage", get(api::get_usage))
        .route("/api/events", get(|state| async move { sse::sse_handler(state) }))
        .route("/", get(index))
        .route("/{*path}", get(serve_static))
        .layer(cors_layer())
        .with_state(store);

    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("[aid] Web UI: http://127.0.0.1:{port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
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
}
