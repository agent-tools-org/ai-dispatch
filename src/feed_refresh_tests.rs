// Shared refresh durability, atomic adoption and in-flight download regressions.
// Tests use isolated cache paths and validators; no live network is needed.
// Deps: feed_refresh, serde_json, tempfile, filesystem metadata.

use super::*;

fn valid(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body).is_ok()
}

#[test]
fn invalid_ready_is_deleted_and_old_cache_stays() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join("cache.json");
    fs::write(&cache, b"old cache").expect("cache");
    fs::write(sibling(&cache, ".ready"), b"invalid JSON").expect("ready");
    maybe_refresh(&cache, "unused", valid, || true);
    assert_eq!(fs::read(&cache).expect("cache"), b"old cache");
    assert!(!sibling(&cache, ".ready").exists());
}

#[test]
fn valid_ready_atomically_replaces_cache_before_freshness_check() {
    use std::io::Read;
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join("cache.json");
    fs::write(&cache, b"old cache").expect("cache");
    let mut old = fs::File::open(&cache).expect("old handle");
    fs::write(sibling(&cache, ".ready"), b"{\"new\":true}").expect("ready");
    maybe_refresh(&cache, "unused", valid, || {
        assert_eq!(fs::read(&cache).expect("new cache"), b"{\"new\":true}");
        true
    });
    let mut bytes = Vec::new();
    old.read_to_end(&mut bytes).expect("old data");
    assert_eq!(bytes, b"old cache");
    assert!(!sibling(&cache, ".ready").exists());
    assert!(!sibling(&cache, ".part").exists());
}

#[test]
fn fresh_part_suppresses_second_spawn_even_with_missing_cache() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join("cache.json");
    let part = sibling(&cache, ".part");
    fs::write(&part, b"download in progress").expect("part");
    assert!(!reserve_part(&part));
    maybe_refresh(&cache, "unused", valid, || false);
    assert_eq!(fs::read(part).expect("part"), b"download in progress");
    assert!(!cache.exists());
    assert!(!sibling(&cache, ".ready").exists());
}

#[test]
fn stale_part_allows_one_new_reservation() {
    let dir = tempfile::tempdir().expect("dir");
    let part = dir.path().join("cache.json.part");
    fs::write(&part, b"abandoned").expect("part");
    let old = std::time::SystemTime::now() - Duration::from_secs(61);
    let times = fs::FileTimes::new().set_modified(old);
    fs::File::open(&part)
        .expect("part")
        .set_times(times)
        .expect("mtime");
    assert!(reserve_part(&part));
    assert!(!reserve_part(&part));
    assert!(fs::read(part).expect("reservation").is_empty());
}

#[test]
fn missing_ready_preserves_cache() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join("cache.json");
    fs::write(&cache, b"old cache").expect("cache");
    maybe_refresh(
        &cache,
        "unused",
        |_| panic!("no response to validate"),
        || true,
    );
    assert_eq!(fs::read(cache).expect("cache"), b"old cache");
}
