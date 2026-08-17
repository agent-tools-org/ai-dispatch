// Parse-only aidbar cache tests. Override policy is pinned in route_availability_tests.

use super::*;
use crate::types::AgentKind;

#[test]
fn provider_mapping_matches_aidbar_probe_ids() {
    assert_eq!(provider_name(&AgentKind::Codex), Some("codex"));
    assert_eq!(provider_name(&AgentKind::Antigravity), Some("agy"));
    assert_eq!(provider_name(&AgentKind::Copilot), None);
    assert_eq!(provider_name(&AgentKind::Grok), Some("grok"));
    assert_eq!(provider_name(&AgentKind::Qwen), Some("qwen"));
}

#[test]
fn usage_window_deserializes_label_resets_at_and_group() {
    let raw = r#"{"ok":true,"snapshot":{"provider":"grok","windows":[{"label":"Aug 11 – Aug 18","used_percent":0.0,"resets_at":"2026-08-18T00:55:28Z","group":null}],"fetched_at":"2026-08-17T12:24:00Z"}}"#;
    let record: CachedRecord = serde_json::from_str(raw).expect("valid aidbar record");
    let probe = probe_from_record(&record, "grok").expect("snapshot");
    assert_eq!(probe.windows[0].label, "Aug 11 – Aug 18");
    assert!(probe.windows[0].resets_at.is_some());
    assert_eq!(probe.windows[0].group, None);
}

#[test]
fn aidbar_error_records_have_no_snapshot_to_override() {
    let raw = r#"{"ok":false,"snapshot":null,"error":"not logged in"}"#;
    let record: CachedRecord = serde_json::from_str(raw).expect("valid aidbar record");
    assert!(probe_from_record(&record, "grok").is_none());
}

#[test]
fn newer_successful_aidbar_cache_record_overrides_marker_from_disk() {
    let temp = tempfile::tempdir().expect("temp directory");
    let marker = temp.path().join("rate-limit-codex");
    let cache_dir = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache_dir).expect("cache directory");
    std::fs::write(
        &marker,
        "recovery_at: Aug 11, 2026 2:23 PM\nmessage: hit your usage limit\n",
    )
    .expect("marker");
    std::fs::write(
        cache_dir.join("codex.json"),
        r#"{"ok":true,"snapshot":{"provider":"codex","plan":"pro","windows":[{"label":"Weekly","used_percent":0.0,"resets_at":null}],"fetched_at":"2099-01-01T00:00:00Z"}}"#,
    )
    .expect("cache record");

    assert!(crate::route_availability::overrides_marker_at_in_cache(
        &AgentKind::Codex,
        &marker,
        &cache_dir,
    ));
}

#[test]
fn newer_opencode_headroom_does_not_release_a_needs_human_marker() {
    let temp = tempfile::tempdir().expect("temp directory");
    let marker = temp.path().join("rate-limit-opencode");
    let cache_dir = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache_dir).expect("cache directory");
    std::fs::write(
        cache_dir.join("opencode.json"),
        r#"{"ok":true,"snapshot":{"provider":"opencode","plan":"zen","windows":[{"label":"5h","used_percent":96.86,"resets_at":null},{"label":"Weekly","used_percent":57.08,"resets_at":null}],"fetched_at":"2099-01-01T00:00:00Z"}}"#,
    )
    .expect("cache record");

    for marker_content in [
        "recovery_at: \nhold: manual\nmessage: Insufficient balance\n",
        "recovery_at: \nmessage: Insufficient balance. Manage your billing here\n",
    ] {
        std::fs::write(&marker, marker_content).expect("marker");
        assert!(!crate::route_availability::overrides_marker_at_in_cache(
            &AgentKind::OpenCode,
            &marker,
            &cache_dir,
        ));
    }
}

#[test]
fn provider_mismatch_or_ok_false_is_not_a_snapshot() {
    let raw = r#"{"ok":true,"snapshot":{"provider":"claude","windows":[{"used_percent":0.0}],"fetched_at":"2099-01-01T00:00:00Z"}}"#;
    let record: CachedRecord = serde_json::from_str(raw).expect("valid");
    assert!(probe_from_record(&record, "codex").is_none());

    let failed = r#"{"ok":false,"snapshot":{"provider":"codex","windows":[{"used_percent":0.0}],"fetched_at":"2099-01-01T00:00:00Z"}}"#;
    let record: CachedRecord = serde_json::from_str(failed).expect("valid");
    let probe = probe_from_record(&record, "codex").expect("parsed");
    assert!(!probe.ok);
}
