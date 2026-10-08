// Cache durability and freshness regressions against the captured relay sample.
// Covers missing/corrupt caches and failed, empty, stale, valid atomic refreshes.
// Deps: feed cache helpers, isolated AidHomeGuard, tempfile.

use super::*;
use crate::{paths::AidHomeGuard, scores::test_support::sample};

#[test]
fn cache_refresh_keeps_old_sample_on_failure_empty_stale_or_malformed() {
    let dir = tempfile::tempdir().expect("dir");
    let _guard = AidHomeGuard::set(dir.path());
    let path = crate::paths::aid_dir().join("scores.json");
    let old = sample();
    store_response(&path, Some(&serde_json::to_vec(&old).expect("sample"))).expect("store");
    for body in [None, Some(b"broken JSON".as_slice())] {
        let _ = store_response(&path, body);
        assert_eq!(load_cache(), Some(old.clone()));
    }
    let mut responses = vec![old.clone(); 3];
    responses[0].models.clear();
    responses[1].stale = Some(true);
    responses[2].age_seconds = Some(TTL_SECONDS + 1);
    for response in responses {
        store_response(
            &path,
            Some(&serde_json::to_vec(&response).expect("response")),
        )
        .expect("store");
        assert_eq!(load_cache(), Some(old.clone()));
    }
    let mut new = old;
    new.built_at = Utc::now().to_rfc3339();
    store_response(&path, Some(&serde_json::to_vec(&new).expect("new"))).expect("store");
    assert_eq!(load_cache(), Some(new));
    assert_eq!(
        fs::read_dir(crate::paths::aid_dir())
            .expect("cache dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count(),
        0
    );
}

#[test]
fn missing_corrupt_or_empty_cache_is_unknown() {
    let dir = tempfile::tempdir().expect("dir");
    let _guard = AidHomeGuard::set(dir.path());
    assert_eq!(load_cache(), None);
    fs::create_dir_all(crate::paths::aid_dir()).expect("dir");
    let path = crate::paths::aid_dir().join("scores.json");
    fs::write(&path, b"bad").expect("cache");
    assert_eq!(load_cache(), None);
    let mut feed = sample();
    feed.models.clear();
    fs::write(path, serde_json::to_vec(&feed).expect("feed")).expect("cache");
    assert_eq!(load_cache(), None);
}

#[test]
fn stale_local_cache_serves_evidence_but_needs_refresh_after_24_hours() {
    let dir = tempfile::tempdir().expect("dir");
    let _guard = AidHomeGuard::set(dir.path());
    let mut feed = sample();
    feed.built_at = (Utc::now() - chrono::Duration::hours(25)).to_rfc3339();
    fs::create_dir_all(crate::paths::aid_dir()).expect("dir");
    fs::write(
        crate::paths::aid_dir().join("scores.json"),
        serde_json::to_vec(&feed).expect("feed"),
    )
    .expect("cache");
    assert!(!fresh(&feed));
    assert_eq!(load_cache(), Some(feed.clone()));
    feed.built_at = Utc::now().to_rfc3339();
    assert!(fresh(&feed));
    feed.stale = Some(true);
    assert!(!fresh(&feed));
}
