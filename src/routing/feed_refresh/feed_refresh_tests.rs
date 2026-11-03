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

#[cfg(target_os = "linux")]
#[test]
fn process_safety_refresh_reaps_shell_without_blocking_caller() {
    let _permit = crate::support::test_subprocess::acquire();
    let dir = tempfile::tempdir().expect("dir");
    let pid_file = dir.path().join("pid");
    let release = dir.path().join("release");
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "echo $$ > \"$1\"; while [ ! -e \"$2\" ]; do sleep 0.01; done",
            "refresh",
        ])
        .arg(&pid_file)
        .arg(&release);
    spawn_refresh(command, dir.path().join("cache.part"));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !pid_file.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid = fs::read_to_string(pid_file).expect("caller returned while shell was running");
    let proc = PathBuf::from(format!("/proc/{}", pid.trim()));
    fs::write(release, b"").expect("release shell");
    while proc.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !proc.exists(),
        "exited refresh shell must be reaped, not left as a zombie"
    );
}

#[test]
fn process_safety_refresh_download_handles_leading_dash_cache_names() {
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("source.json");
    fs::write(&source, b"{}").expect("source");
    let status = Command::new("sh")
        .current_dir(dir.path())
        .args(["-c", DOWNLOAD, "refresh", "-cache"])
        .arg(format!("file://{}", source.display()))
        .status()
        .expect("download");
    assert!(
        status.success(),
        "mv must treat the cache name as an operand"
    );
    assert_eq!(
        fs::read(dir.path().join("-cache.ready")).expect("ready"),
        b"{}"
    );
}

#[test]
fn process_safety_refresh_download_rejects_oversized_feed() {
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("large.json");
    fs::File::create(&source)
        .expect("source")
        .set_len(10485761)
        .expect("size");
    let status = Command::new("sh")
        .current_dir(dir.path())
        .args(["-c", DOWNLOAD, "refresh", "cache"])
        .arg(format!("file://{}", source.display()))
        .status()
        .expect("download");
    assert!(
        !status.success(),
        "curl must reject feeds larger than 10 MiB"
    );
    assert!(!dir.path().join("cache.ready").exists());
}
