// EverMemOS memory cloud HTTP client helpers.
// Exports the EverMemosClient, config, and metadata types for other modules.
// Depends on ureq, anyhow, serde, serde_json, and chrono for timestamps.
#![allow(dead_code)]
use anyhow::{anyhow, Context, Error, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ureq::RequestBuilder;
const DEFAULT_MEMORY_TYPES: [&str; 1] = ["episodic"];
const DEFAULT_RETRIEVE_METHOD: &str = "hybrid";
#[derive(Debug, Clone, Serialize)]
pub struct MemoryMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub memory_type: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct CloudMemory {
    pub content: String,
    pub score: f64,
    pub metadata: Value,
}
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EverMemosConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    pub api_key: Option<String>,
    #[serde(default = "default_user_id")]
    pub user_id: String,
}
pub fn default_base_url() -> String {
    "http://localhost:1995/api/v1".to_string()
}
pub fn default_user_id() -> String {
    "aid-user".to_string()
}
pub struct EverMemosClient {
    base_url: String,
    api_key: Option<String>,
    user_id: String,
}
impl EverMemosClient {
    pub fn new(base_url: &str, api_key: Option<&str>, user_id: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        Self {
            base_url: base,
            api_key: api_key.map(str::to_string),
            user_id: user_id.to_string(),
        }
    }
    pub fn from_config(config: &EverMemosConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }
        Some(Self::new(
            &config.base_url,
            config.api_key.as_deref(),
            &config.user_id,
        ))
    }
    pub fn health_check(&self) -> Result<bool> {
        let url = self.build_url("health");
        let request = apply_api_key(self.agent().get(&url), self.api_key.as_deref());
        let response = request
            .call()
            .map_err(Error::new)
            .context("performing health check")
            .map_err(|err| {
                log_error("health check request failed", &err);
                err
            })?;
        let (_, body) = response.into_parts();
        let status: HealthResponse = serde_json::from_reader(body.into_reader())
            .map_err(Error::new)
            .context("decoding health check")
            .map_err(|err| {
                log_error("health check response invalid", &err);
                err
            })?;
        Ok(status.status == "healthy")
    }
    pub fn store_memory(&self, content: &str, metadata: &MemoryMetadata) -> Result<()> {
        let url = self.build_url("memories");
        let metadata_value = log_result(
            serde_json::to_value(metadata)
                .map_err(Error::new)
                .context("serializing memory metadata"),
            "serializing memory metadata failed",
        )?;
        let now = Utc::now();
        let message_id = format!("{}-{}", &self.user_id, now.timestamp_micros());
        let payload = json!({
            "message_id": message_id,
            "create_time": now.to_rfc3339_opts(SecondsFormat::Secs, true),
            "sender": &self.user_id,
            "content": content,
            "metadata": metadata_value,
        });
        let request = apply_api_key(self.agent().post(&url), self.api_key.as_deref());
        let response = request
            .send_json(&payload)
            .map_err(Error::new)
            .context("sending memory store request")
            .map_err(|err| {
                log_error("memory store request failed", &err);
                err
            })?;
        let (_, body) = response.into_parts();
        let status: StatusResponse = serde_json::from_reader(body.into_reader())
            .map_err(Error::new)
            .context("decoding memory store response")
            .map_err(|err| {
                log_error("memory store response invalid", &err);
                err
            })?;
        if status.status.to_lowercase() != "ok" {
            let err = anyhow!("EverMemOS returned unexpected status: {}", status.status);
            log_error("memory store returned bad status", &err);
            return Err(err);
        }
        Ok(())
    }
    pub fn search_memories(&self, query: &str, top_k: usize) -> Result<Vec<CloudMemory>> {
        let url = self.build_url("memories/search");
        let body = json!({
            "query": query,
            "user_id": &self.user_id,
            "memory_types": DEFAULT_MEMORY_TYPES,
            "retrieve_method": DEFAULT_RETRIEVE_METHOD,
            "top_k": top_k,
        });
        let request = apply_api_key(self.agent().get(&url), self.api_key.as_deref());
        let response = request
            .force_send_body()
            .send_json(body)
            .map_err(Error::new)
            .context("sending memory search request")
            .map_err(|err| {
                log_error("memory search request failed", &err);
                err
            })?;
        let (_, body) = response.into_parts();
        let results: SearchResponse = serde_json::from_reader(body.into_reader())
            .map_err(Error::new)
            .context("decoding memory search response")
            .map_err(|err| {
                log_error("memory search response invalid", &err);
                err
            })?;
        Ok(results.memories)
    }
    fn agent(&self) -> ureq::Agent {
        ureq::Agent::new_with_defaults()
    }
    fn build_url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        format!("{}/{}", base, path)
    }

}
#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}
#[derive(Debug, Deserialize)]
struct StatusResponse {
    status: String,
}
#[derive(Debug, Deserialize)]
struct SearchResponse {
    memories: Vec<CloudMemory>,
}
fn apply_api_key<B>(request: RequestBuilder<B>, api_key: Option<&str>) -> RequestBuilder<B> {
    if let Some(key) = api_key {
        request.header("Authorization", &format!("Bearer {key}"))
    } else {
        request
    }
}
fn log_result<T>(result: Result<T, anyhow::Error>, context: &str) -> Result<T> {
    result.map_err(|err| {
        log_error(context, &err);
        err
    })
}
fn log_error(context: &str, error: &anyhow::Error) {
    eprintln!("[aid] EverMemOS: {context}: {error}");
}
