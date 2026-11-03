// Degraded-route dispatch: warn and still send work; never substitute.
// Deps: resolve_agent_setup, degraded_dispatch_warning, probe fixtures.

use super::super::degraded::degraded_dispatch_warning;
use super::*;
use crate::routing::quota::live::CacheDirGuard;
use crate::support::paths::{self, AidHomeGuard};
use chrono::Utc;

fn isolated_codex_probe(used: f64) -> (tempfile::TempDir, AidHomeGuard, CacheDirGuard) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _ = std::fs::create_dir_all(tmp.path().join(".aid"));
    let home = AidHomeGuard::set(tmp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();
    let aidbar = tmp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).expect("cache");
    let fetched = Utc::now().to_rfc3339();
    std::fs::write(
        aidbar.join("codex.json"),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"codex","windows":[{{"label":"Weekly","used_percent":{used},"resets_at":"2026-09-19T08:10:00Z"}}],"fetched_at":"{fetched}"}}}}"#
        ),
    )
    .expect("cache");
    let cache = CacheDirGuard::set(&aidbar);
    (tmp, home, cache)
}

#[test]
fn degraded_route_warns_and_does_not_divert() {
    let (_tmp, _home, _cache) = isolated_codex_probe(96.0);
    assert_eq!(
        degraded_dispatch_warning(AgentKind::Codex, None, None).as_deref(),
        Some(
            "[aid] codex route degraded: 96% of Weekly window used (resets 2026-09-19T08:10Z) — dispatching anyway"
        )
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Add unit tests".to_string(),
        cascade: vec!["oz".to_string()],
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("dispatch degraded route");
    assert_eq!(setup.agent_kind, AgentKind::Codex);
    assert!(setup.substituted_from.is_none());
    assert_eq!(args.cascade, vec!["oz".to_string()]);
}

#[test]
fn ok_route_at_20_percent_does_not_warn_or_divert() {
    let (_tmp, _home, _cache) = isolated_codex_probe(20.0);
    assert_eq!(
        degraded_dispatch_warning(AgentKind::Codex, None, None),
        None
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Add unit tests".to_string(),
        cascade: vec!["oz".to_string()],
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("dispatch ok route");
    assert_eq!(setup.agent_kind, AgentKind::Codex);
    assert!(setup.substituted_from.is_none());
}

#[test]
fn agy_degradation_uses_only_resolved_pool() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(home.path());
    let now = Utc::now();
    std::fs::write(
        home.path().join("agy.json"),
        serde_json::json!({
            "ok": true, "snapshot": {"provider": "agy", "fetched_at": now.to_rfc3339(),
            "windows": [
                {"label": "Gemini 4 Argon Weekly Limit", "used_percent": 100.0,
                 "resets_at": (now + chrono::Duration::hours(36)).to_rfc3339()},
                {"label": "Claude / GPT Weekly", "group": "claude-gpt", "used_percent": 20.0}
            ]}
        })
        .to_string(),
    )
    .expect("probe");
    let model = Some("claude-opus-5-5-medium");
    assert_eq!(
        degraded_dispatch_warning(AgentKind::Antigravity, None, model),
        None
    );
    let path = home.path().join("agy.json");
    let text = std::fs::read_to_string(&path).expect("read probe");
    std::fs::write(&path, text.replace("20.0", "90.0")).expect("degraded probe");
    let warning = degraded_dispatch_warning(AgentKind::Antigravity, None, model).expect("warning");
    assert!(warning.contains("90% of Claude / GPT Weekly"));
    assert!(!warning.contains("100%"));
}
