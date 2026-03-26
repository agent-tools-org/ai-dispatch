// Update notification cache and crates.io refresh for the aid CLI.
// Exports maybe_check_update(); depends on chrono, serde, serde_json, and paths.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

use crate::paths;

const CACHE_FILE: &str = "update-check.json";
const CRATE_URL: &str = "https://crates.io/api/v1/crates/ai-dispatch";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct UpdateCache {
    last_check: String,
    latest_version: String,
}

pub fn maybe_check_update() {
    let path = paths::aid_dir().join(CACHE_FILE);
    let cache = read_cache(&path).ok();
    let fresh = cache.as_ref().is_some_and(|value| checked_recently(&value.last_check));
    if !fresh {
        let path = path.clone();
        let _ = std::thread::Builder::new().name("aid-update-check".into()).spawn(move || refresh_cache(&path));
    }
    let latest = cache.map(|value| value.latest_version);
    if let Some(latest) = latest
        && version_is_newer(env!("CARGO_PKG_VERSION"), &latest)
    {
        let current = env!("CARGO_PKG_VERSION");
        let msg = format!("Update available: v{current} → v{latest}");
        let cmd = "Run:  aid upgrade";
        let width = msg.len().max(cmd.len()) + 4;
        aid_warn!("");
        aid_warn!("  ╭{}╮", "─".repeat(width));
        aid_warn!("  │  {:<w$}│", msg, w = width - 2);
        aid_warn!("  │  {:<w$}│", cmd, w = width - 2);
        aid_warn!("  ╰{}╯", "─".repeat(width));
        aid_warn!("");
    }
}

fn refresh_cache(path: &Path) {
    let Ok(output) = Command::new("curl").args(["-s", CRATE_URL]).output() else { return };
    if !output.status.success() {
        return;
    }
    let Ok(body) = String::from_utf8(output.stdout) else { return };
    let Some(latest_version) = parse_crates_io_response(&body) else { return };
    let cache = UpdateCache {
        last_check: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        latest_version,
    };
    let _ = write_cache(path, &cache);
}

fn read_cache(path: &Path) -> Result<UpdateCache, ()> {
    serde_json::from_slice(&fs::read(path).map_err(|_| ())?).map_err(|_| ())
}

fn write_cache(path: &Path, cache: &UpdateCache) -> Result<(), ()> {
    fs::write(path, serde_json::to_vec(cache).map_err(|_| ())?).map_err(|_| ())
}

fn parse_crates_io_response(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("crate")?
        .get("max_version")?
        .as_str()
        .map(str::to_string)
}

fn checked_recently(last_check: &str) -> bool {
    DateTime::parse_from_rfc3339(last_check)
        .ok()
        .map(|value| Utc::now().signed_duration_since(value.with_timezone(&Utc)) < Duration::hours(24))
        .unwrap_or(false)
}

fn version_is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

fn parse_version(value: &str) -> Option<[u32; 3]> {
    let mut parts = value.split('.').map(|part| part.parse::<u32>().ok());
    Some([parts.next()??, parts.next()??, parts.next()??])
}

#[cfg(test)]
mod tests {
    use super::{UpdateCache, maybe_check_update, parse_crates_io_response, read_cache, version_is_newer, write_cache};
    use crate::paths::AidHomeGuard;
    use tempfile::TempDir;

    #[test]
    fn version_is_newer_uses_semver_parts() {
        assert!(version_is_newer("8.16.0", "8.17.0"));
        assert!(version_is_newer("8.16.9", "8.17.0"));
        assert!(!version_is_newer("8.16.0", "8.16.0"));
        assert!(!version_is_newer("8.17.0", "8.16.9"));
    }

    #[test]
    fn cache_round_trip_reads_and_writes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("update-check.json");
        let cache = UpdateCache {
            last_check: "2026-03-18T00:00:00Z".to_string(),
            latest_version: "8.17.0".to_string(),
        };

        write_cache(&path, &cache).unwrap();

        assert_eq!(read_cache(&path).unwrap(), cache);
    }

    #[test]
    fn parse_crates_io_response_extracts_max_version() {
        let body = r#"{"crate":{"id":"ai-dispatch","max_version":"8.17.0"}}"#;

        assert_eq!(parse_crates_io_response(body).as_deref(), Some("8.17.0"));
    }

    #[test]
    fn maybe_check_update_uses_fresh_cache_without_panicking() {
        let dir = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(dir.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        write_cache(
            &dir.path().join("update-check.json"),
            &UpdateCache {
                last_check: "3026-03-18T00:00:00Z".to_string(),
                latest_version: env!("CARGO_PKG_VERSION").to_string(),
            },
        )
        .unwrap();

        maybe_check_update();
    }
}
