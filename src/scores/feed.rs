// Cached third-party leaderboard relay: local reads and out-of-band refresh.
// Exports: Feed, Score, Source, load_cache, maybe_refresh.
// Deps: aid paths, chrono, serde, curl; atomic cache replacement via rename.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const URL: &str = "https://llm-prices.agent-tools.org/v1/scores.json";
const TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Source {
    pub ok: bool,
    pub count: usize,
    pub updated_at: Option<String>,
    pub url: String,
    pub licence: String,
    pub attribution: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Score {
    pub source: String,
    pub board: String,
    pub cli: Option<String>,
    pub effort: Option<String>,
    pub value: f64,
    pub unit: String,
    pub rank: Option<usize>,
    pub of: Option<usize>,
    pub ci_low: Option<f64>,
    pub ci_high: Option<f64>,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Model {
    pub id: String,
    pub scores: Vec<Score>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Unmapped {
    pub source: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Feed {
    pub built_at: String,
    pub age_seconds: Option<i64>,
    pub stale: Option<bool>,
    pub sources: BTreeMap<String, Source>,
    pub models: Vec<Model>,
    pub unmapped: Vec<Unmapped>,
}

pub(crate) fn load_cache() -> Option<Feed> {
    let bytes = fs::read(crate::paths::aid_dir().join("scores.json")).ok()?;
    let feed: Feed = serde_json::from_slice(&bytes).ok()?;
    (!feed.models.is_empty()).then_some(feed)
}

fn fresh(feed: &Feed) -> bool {
    let Ok(built) = DateTime::parse_from_rfc3339(&feed.built_at) else {
        return false;
    };
    let age = Utc::now().signed_duration_since(built).num_seconds();
    usable(feed) && (0..TTL_SECONDS).contains(&age)
}

fn usable(feed: &Feed) -> bool {
    !feed.models.is_empty()
        && feed.stale == Some(false)
        && feed
            .age_seconds
            .is_some_and(|age| (0..=TTL_SECONDS).contains(&age))
}

// Failed, malformed, empty, or server-stale responses never replace an old cache.
fn store_response(path: &Path, body: Option<&[u8]>) -> anyhow::Result<()> {
    let Some(body) = body else { return Ok(()) };
    let feed: Feed = serde_json::from_slice(body)?;
    if !usable(&feed) {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cache has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec(&feed)?)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn maybe_refresh() {
    if load_cache().is_some_and(|feed| fresh(&feed)) {
        return;
    }
    let path = crate::paths::aid_dir().join("scores.json");
    let _ = std::thread::Builder::new()
        .name("aid-scores-feed".into())
        .spawn(move || {
            let response = Command::new("curl")
                .args(["-sfL", "--max-time", "15", URL])
                .output();
            let body = response
                .ok()
                .filter(|out| out.status.success())
                .map(|out| out.stdout);
            let _ = store_response(&path, body.as_deref());
        });
}

#[cfg(test)]
#[path = "feed_tests.rs"]
mod tests;
