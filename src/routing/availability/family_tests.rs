// Family refusals use explicit recovery dates and ignore undated markers.
// Uses isolated homes for untrusted marker and live-window evidence.
use super::*;

#[test]
fn agy_family_requires_dated_untrusted_recovery() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = live_quota::CacheDirGuard::set(home.path());
    let path = group_marker_path(&AgentKind::Antigravity, None, "gemini");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(
        &path,
        "source: untrusted\nmessage: Individual quota reached. Resets in 36h29m20s.\n",
    )
    .expect("legacy marker");
    assert!(!family_is_spent(AgentKind::Antigravity, None, "gemini"));
    for (hours, spent) in [(36, true), (-1, false)] {
        let at = Local::now().naive_local() + chrono::Duration::hours(hours);
        std::fs::write(
            &path,
            format!(
                "source: untrusted\nrecovery_at: {}\n",
                crate::routing::rate_limit::format_recovery(at)
            ),
        )
        .expect("dated marker");
        assert_eq!(
            family_is_spent(AgentKind::Antigravity, None, "gemini"),
            spent
        );
    }
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
