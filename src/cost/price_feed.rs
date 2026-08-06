// Live model price feed (https://llm-prices.agent-tools.org/v1/prices.json),
// cached under the aid home. Feed prices take precedence over the built-in
// matcher; the cache is refreshed out of band so the dispatch path never
// blocks on the network.
// Exports: Feed, feed_lookup, maybe_refresh
// Deps: crate::paths, chrono, serde, serde_json, std::process::Command (curl)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::time::Duration;

const FEED_URL: &str = "https://llm-prices.agent-tools.org/v1/prices.json";
const CACHE_FILE: &str = "prices.json";
/// How long a locally cached feed is trusted before a refresh is attempted.
/// 24h matches the update-check cache: model prices move on a daily cadence and
/// a day-old price is never materially wrong for an estimate. `stale` and
/// `age_seconds` from the server guard the *content* of the cache — a stale
/// feed is never written over a fresh one.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// An excessively old feed (`age_seconds` from the server) is not trusted.
const MAX_SERVER_AGE: i64 = 24 * 60 * 60;

/// A canonical priced model from the feed.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FeedModel {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cached_input_per_mtok: Option<f64>,
    pub context_length: Option<i64>,
    pub source: Option<String>,
}

/// The feed envelope. `stale` and `age_seconds` are produced by the server and
/// we trust them over re-deriving freshness locally.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Feed {
    pub built_at: String,
    pub age_seconds: Option<i64>,
    pub stale: Option<bool>,
    pub count: Option<i64>,
    pub models: Vec<FeedModel>,
}

impl Feed {
    /// Index the feed once: canonical ids and per-CLI aliases -> canonical id.
    pub fn index(&self) -> HashMap<String, usize> {
        let mut index = HashMap::with_capacity(self.models.len() * 2);
        for (i, model) in self.models.iter().enumerate() {
            index.insert(model.id.clone(), i);
            for alias in &model.aliases {
                index.insert(alias.clone(), i);
            }
        }
        index
    }

    /// The feed is usable when the server says so and the server-reported age
    /// is within bounds. A stale or excessively old feed must never be written
    /// over a fresh cache.
    pub fn usable(&self) -> bool {
        if self.stale.unwrap_or(true) {
            return false;
        }
        self.age_seconds.unwrap_or(i64::MAX) <= MAX_SERVER_AGE
    }
}

/// Resolve `model` to a priced feed entry. Exact canonical id, then alias, then
/// the vendor-stripped basename (and its aliases). A model the feed does not
/// carry is a miss — there is no substring matching or near-match fallback.
pub fn feed_lookup<'a>(feed: &'a Feed, index: &HashMap<String, usize>, model: &str) -> Option<&'a FeedModel> {
    if let Some(&i) = index.get(model) {
        return feed.models.get(i);
    }
    let basename = model.rsplit('/').next().unwrap_or(model);
    if basename != model
        && let Some(&i) = index.get(basename)
    {
        return feed.models.get(i);
    }
    None
}

/// Where the refresh result is written. Cache lives under the aid home so a
/// cold or stale cache never delays a run — serving is a local file read.
fn cache_path() -> std::path::PathBuf {
    crate::paths::aid_dir().join(CACHE_FILE)
}

/// Read the cached feed, if any. Errors are swallowed: a missing or corrupt
/// cache is an offline first run, which falls back to the built-in matcher.
pub fn load_cache() -> Option<Feed> {
    let bytes = fs::read(cache_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The feed is fresh enough to serve without a refresh attempt.
pub fn cache_fresh(feed: &Feed) -> bool {
    let Ok(built) = DateTime::parse_from_rfc3339(&feed.built_at) else {
        return false;
    };
    Utc::now().signed_duration_since(built.with_timezone(&Utc)) < chrono::Duration::from_std(CACHE_TTL).unwrap_or_default()
}

/// Out-of-band refresh: fetch the feed and replace the cache only when the new
/// feed is fresh. A network failure keeps the old cache ("keep the old cache",
/// never "no prices"). Never blocks or fails a run.
pub fn maybe_refresh() {
    if load_cache().is_some_and(|feed| cache_fresh(&feed)) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("aid-price-feed".into())
        .spawn(|| {
            let Ok(output) = Command::new("curl").args(["-sfL", "--max-time", "15", FEED_URL]).output() else {
                eprintln!("[price-feed] curl spawn failed");
                return;
            };
            if !output.status.success() {
                eprintln!("[price-feed] curl status {}", output.status);
                return;
            }
            let Ok(body) = String::from_utf8(output.stdout) else {
                eprintln!("[price-feed] utf8 failed");
                return;
            };
            let Ok(feed) = serde_json::from_str::<Feed>(&body) else {
                eprintln!("[price-feed] json parse failed, len {}", body.len());
                return;
            };
            if !feed.usable() {
                eprintln!("[price-feed] not usable: stale={:?} age={:?}", feed.stale, feed.age_seconds);
                return;
            }
            let Ok(()) = std::fs::create_dir_all(crate::paths::aid_dir()) else {
                eprintln!("[price-feed] create_dir_all failed");
                return;
            };
            let Ok(encoded) = serde_json::to_vec(&feed) else {
                eprintln!("[price-feed] serialize failed");
                return;
            };
            let write_res = fs::write(cache_path(), encoded);
            eprintln!("[price-feed] write result: {:?}", write_res);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;

    fn sample_feed() -> Feed {
        Feed {
            built_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            age_seconds: Some(60),
            stale: Some(false),
            count: Some(2),
            models: vec![
                FeedModel {
                    id: "openai/gpt-5.3-codex".to_string(),
                    aliases: vec!["gpt-5.3-codex-high".to_string()],
                    input_per_mtok: 1.75,
                    output_per_mtok: 14.0,
                    cached_input_per_mtok: None,
                    context_length: None,
                    source: Some("openrouter".to_string()),
                },
                FeedModel {
                    id: "gpt-5.6-sol".to_string(),
                    aliases: vec![],
                    input_per_mtok: 5.0,
                    output_per_mtok: 30.0,
                    cached_input_per_mtok: None,
                    context_length: None,
                    source: Some("openrouter".to_string()),
                },
            ],
        }
    }

    #[test]
    fn lookup_exact_id_and_alias() {
        let feed = sample_feed();
        let index = feed.index();
        assert_eq!(feed_lookup(&feed, &index, "openai/gpt-5.3-codex").unwrap().id, "openai/gpt-5.3-codex");
        assert_eq!(feed_lookup(&feed, &index, "gpt-5.3-codex-high").unwrap().id, "openai/gpt-5.3-codex");
        assert_eq!(feed_lookup(&feed, &index, "gpt-5.6-sol").unwrap().id, "gpt-5.6-sol");
    }

    #[test]
    fn lookup_strips_vendor_prefix() {
        let feed = sample_feed();
        let index = feed.index();
        assert_eq!(feed_lookup(&feed, &index, "mimo/gpt-5.6-sol").unwrap().id, "gpt-5.6-sol");
    }

    #[test]
    fn lookup_miss_is_none() {
        let feed = sample_feed();
        let index = feed.index();
        assert!(feed_lookup(&feed, &index, "nobody/has-this-model").is_none());
        assert!(feed_lookup(&feed, &index, "gpt-5.3-codex-high-extra").is_none());
    }

    #[test]
    fn stale_feed_is_not_usable() {
        let mut feed = sample_feed();
        feed.stale = Some(true);
        assert!(!feed.usable());
        feed.stale = Some(false);
        feed.age_seconds = Some(MAX_SERVER_AGE + 1);
        assert!(!feed.usable());
    }

    #[test]
    fn cache_round_trip_under_aid_home() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(crate::paths::aid_dir()).unwrap();
        let feed = sample_feed();
        fs::write(cache_path(), serde_json::to_vec(&feed).unwrap()).unwrap();
        assert_eq!(load_cache().unwrap(), feed);
        assert!(cache_fresh(&feed));
    }

    #[test]
    fn missing_cache_loads_none() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        assert!(load_cache().is_none());
    }

    #[test]
    #[ignore = "live network probe"]
    fn live_refresh_writes_cache() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        maybe_refresh();
        std::thread::sleep(std::time::Duration::from_secs(20));
        let feed = load_cache().expect("cache should be written after live refresh");
        assert!(feed.usable());
    }
}
