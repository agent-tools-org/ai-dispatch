// Family refusal expiry is measured from marker creation, never rebased on reads.
// Uses isolated homes and filesystem timestamps for legacy untrusted markers.
use super::*;

#[test]
fn agy_family_legacy_untrusted_duration_expires_from_marker_time() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = live_quota::CacheDirGuard::set(home.path());
    let path = group_marker_path(&AgentKind::Antigravity, None, "gemini");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(
        &path,
        "source: untrusted\nmessage: Individual quota reached. Resets in 36h29m20s.\n",
    )
    .expect("legacy marker");
    assert!(family_is_spent(AgentKind::Antigravity, None, "gemini"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("file");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(37 * 3600);
    file.set_times(std::fs::FileTimes::new().set_modified(old))
        .expect("mtime");
    assert!(!family_is_spent(AgentKind::Antigravity, None, "gemini"));
    assert!(!family_is_spent(AgentKind::Antigravity, None, "claude"));
}

#[test]
fn agy_argon_window_does_not_exhaust_flash_but_general_window_exhausts_both() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = live_quota::CacheDirGuard::set(home.path());
    for (label, flash_spent) in [
        ("Gemini 4 Argon Weekly Limit", false),
        ("Gemini Models Weekly", true),
    ] {
        let now = chrono::Utc::now();
        std::fs::write(
            home.path().join("agy.json"),
            serde_json::json!({
                "ok": true, "snapshot": {"provider": "agy", "fetched_at": now.to_rfc3339(),
                "windows": [{"label": label, "group": "gemini", "used_percent": 100.0,
                "resets_at": (now + chrono::Duration::hours(36)).to_rfc3339()}]}
            })
            .to_string(),
        )
        .expect("probe");
        assert!(family_is_spent(
            AgentKind::Antigravity,
            None,
            "gemini-4-argon"
        ));
        assert_eq!(
            family_is_spent(AgentKind::Antigravity, None, "gemini"),
            flash_spent
        );
    }
}
