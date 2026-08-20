// Tests agent JSON contracts, discovered metadata, and quota marker rendering.
// Exports: module-scoped tests only.
// Deps: agent_json builders, Store, isolated AID homes, serde_json.

use std::collections::HashMap;
use crate::agent::custom::CustomAgentConfig;
use crate::cmd::agent_json_types::{
    AgentListJson, AgentJson, GroupHoldJson, QuotaJson, ModelsJson, AvailableModelJson,
    HistoryJson, CategoryHistoryJson, LoadJson,
};
use crate::types::AgentKind;
use super::{build_quota_json, get_agents_list, rate_limit_kind};
use crate::cmd::agent_json_helpers::custom_has_endpoint;

#[test]
fn test_agent_json_serialization_roundtrip() {
    let mut capabilities = HashMap::new();
    capabilities.insert("research".to_string(), 9);
    capabilities.insert("simple-edit".to_string(), 2);

    let mut by_category = HashMap::new();
    by_category.insert("simple-edit".to_string(), CategoryHistoryJson {
        tasks: 210,
        success_rate: 0.83,
        avg_duration_secs: Some(402.0),
    });

    let agent = AgentJson {
        name: "codex".to_string(),
        kind: "builtin".to_string(),
        installed: true,
        disabled: false,
        trust_tier: "third-party".to_string(),
        description: "Complex implementation, multi-file refactors".to_string(),
        supports_session_resume: true,
        provider: "openai-chatgpt-plan".to_string(),
        metering: "account_pool".to_string(),
        quota: QuotaJson {
            state: "limited".to_string(),
            recovery_at: Some("2026-08-05T18:27:00+08:00".to_string()),
            message: Some("You have hit your usage limit...".to_string()),
            source: "marker".to_string(),
            groups: vec![],
        },
        capabilities,
        models: ModelsJson {
            default: None,
            budget: Some("gpt-5.4-mini".to_string()),
            available: vec![AvailableModelJson {
                model: "gpt-5.5".to_string(),
                tier: "paid".to_string(),
                input_per_m: Some(1.25),
                output_per_m: Some(10.0),
                capability: Some(9.6),
            }],
        },
        history: Some(HistoryJson {
            window_days: 30,
            tasks: 1374,
            success_rate: 0.78,
            avg_duration_secs: Some(589.0),
            avg_cost_usd: Some(20.16),
            by_category,
        }),
        load: LoadJson { running: 2 },
    };

    let json_str = serde_json::to_string(&agent).unwrap();
    let deserialized: AgentJson = serde_json::from_str(&json_str).unwrap();
    assert_eq!(agent, deserialized);
}

#[test]
fn test_agent_list_json_serialization_roundtrip() {
    let list = AgentListJson {
        generated_at: "2026-08-05T14:02:11+08:00".to_string(),
        agents: vec![],
    };
    let json_str = serde_json::to_string(&list).unwrap();
    let deserialized: AgentListJson = serde_json::from_str(&json_str).unwrap();
    assert_eq!(list, deserialized);
}

#[test]
fn agent_list_marks_discovered_agy_metadata_unknown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("aid dirs");
    crate::agent::model_validation::clear_served_models_cache();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let cache = serde_json::json!({
        "agy": {"models": ["gemini-3.7-flash-high"], "updated_at_secs": now}
    });
    std::fs::write(
        crate::paths::aid_dir().join("served_models_cache.json"),
        cache.to_string(),
    )
    .expect("served-model cache");

    let store = crate::store::Store::open_memory().expect("store");
    let list = get_agents_list(&store).expect("agent list");
    let agy = list.agents.iter().find(|agent| agent.name == "agy").expect("agy");
    let model = agy.models.available.iter()
        .find(|model| model.model == "gemini-3.7-flash-high")
        .expect("discovered model");
    assert_eq!(model.input_per_m, None);
    assert_eq!(model.output_per_m, None);
    assert_eq!(model.capability, None);
}

fn sample_custom_config() -> CustomAgentConfig {
    CustomAgentConfig {
        id: "ollama".into(),
        display_name: "Ollama".into(),
        command: "bash".into(),
        prompt_mode: "arg".into(),
        prompt_flag: String::new(),
        dir_flag: String::new(),
        model_flag: String::new(),
        output_flag: String::new(),
        fixed_args: Vec::new(),
        streaming: true,
        interactive_input: true,
        output_format: "jsonl".into(),
        capabilities: Default::default(),
        trust_tier: "api".into(),
        base_url: Some("http://10.0.32.184:11434/v1".into()),
        provider: Some("ollama".into()),
        metering: Some("none".into()),
        strengths: Vec::new(),
        delegate_to: Some("opencode".into()),
        forced_model: Some("ollama/qwen3:4b".into()),
        binary: None,
        extra_args: Vec::new(),
        rate_limit_kind: None,
    }
}

#[test]
fn custom_with_endpoint_uses_declared_provider_not_builtin_unknown() {
    let config = sample_custom_config();
    let (provider, metering) = crate::types::provider_for_custom(
        config.provider.as_deref(),
        config.metering.as_deref(),
    );
    assert_eq!(provider.as_str(), "ollama");
    assert_eq!(metering, crate::types::MeteringShape::None);
}

#[test]
fn delegate_opencode_with_own_endpoint_does_not_inherit_opencode_rate_limit() {
    let config = sample_custom_config();
    assert!(custom_has_endpoint(&config));
    assert_eq!(
        rate_limit_kind(AgentKind::Custom, Some(&config)),
        AgentKind::Custom
    );
}

#[test]
fn delegate_opencode_without_endpoint_uses_own_custom_marker() {
    // The write path marks (Custom, Some(id)) even when delegate_to = opencode
    // and there is no base_url.  The read must use the same slot.
    let mut config = sample_custom_config();
    config.base_url = None;
    assert!(!custom_has_endpoint(&config));
    assert_eq!(
        rate_limit_kind(AgentKind::Custom, Some(&config)),
        AgentKind::Custom
    );
}

fn isolated_home() -> (
    tempfile::TempDir,
    crate::paths::AidHomeGuard,
    crate::live_quota::CacheDirGuard,
) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _ = std::fs::create_dir_all(tmp.path().join(".aid"));
    let guard = crate::paths::AidHomeGuard::set(tmp.path());
    std::fs::create_dir_all(crate::paths::aid_dir()).ok();
    let aidbar = tmp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).ok();
    let cache = crate::live_quota::CacheDirGuard::set(&aidbar);
    (tmp, guard, cache)
}

/// Guard: if `rate_limit_kind` ever returns `OpenCode` for a `delegate_to`
/// agent again, this test will read `rate-limit-opencode` (empty) and return
/// "ok" instead of "limited", failing the assertion.
#[test]
fn custom_agent_quota_read_matches_write() {
    let (_tmp, _guard, _cache) = isolated_home();
    let mut config = sample_custom_config();
    config.id = "auditor".into();
    config.base_url = None; // no endpoint — triggers the old delegate_to path
    let stated = crate::rate_limit::test_future_recovery_time();
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Custom,
        Some("auditor"),
        &format!("try again at {stated}."),
    );
    let rlk = rate_limit_kind(AgentKind::Custom, Some(&config));
    let q = build_quota_json(&rlk, Some("auditor"));
    assert_eq!(
        q.state, "limited",
        "build_quota_json must read the same marker the write path wrote; \
         state 'ok' means rate_limit_kind has diverged from the write path again"
    );
}

#[test]
fn quota_json_ok_when_no_markers() {
    let (_tmp, _guard, _cache) = isolated_home();
    let q = build_quota_json(&AgentKind::Gemini, None);
    assert_eq!(q.state, "ok");
    assert!(q.groups.is_empty());
    assert!(q.recovery_at.is_none());
    assert!(q.message.is_none());
}

#[test]
fn quota_json_limited_for_agent_level_hold() {
    let (_tmp, _guard, _cache) = isolated_home();
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::rate_limit::test_future_recovery_time()
        ),
    );
    let q = build_quota_json(&AgentKind::Codex, None);
    assert_eq!(q.state, "limited");
    assert!(q.groups.is_empty());
    assert!(q.recovery_at.is_some(), "recovery_at must be set: {q:?}");
}

#[test]
fn quota_json_partial_for_group_hold_carries_provider_message() {
    let (_tmp, _guard, _cache) = isolated_home();
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Cursor,
        None,
        "premium",
        "ActionRequiredError: You're out of usage. Switch to Auto, or ask your admin to increase your limit",
    );
    // The agent-level marker must NOT be set.
    assert!(!crate::rate_limit::is_rate_limited(&AgentKind::Cursor, None));
    let q = build_quota_json(&AgentKind::Cursor, None);
    assert_eq!(q.state, "partial");
    assert_eq!(q.recovery_at, None);
    assert_eq!(q.message, None);
    assert_eq!(q.groups.len(), 1);
    let hold = &q.groups[0];
    assert_eq!(hold.group, "premium");
    assert!(
        hold.message.as_deref().unwrap_or("").contains("admin"),
        "provider message must be carried: {hold:?}"
    );
}

#[test]
fn quota_json_partial_omitted_from_serialized_ok_output() {
    // `groups` must not appear in JSON when state is "ok".
    let q = QuotaJson {
        state: "ok".to_string(),
        recovery_at: None,
        message: None,
        source: "marker".to_string(),
        groups: vec![],
    };
    let json = serde_json::to_string(&q).expect("serialize");
    assert!(!json.contains("groups"), "empty groups must be omitted: {json}");
}

#[test]
fn quota_json_partial_groups_roundtrip() {
    let q = QuotaJson {
        state: "partial".to_string(),
        recovery_at: None,
        message: None,
        source: "marker".to_string(),
        groups: vec![GroupHoldJson {
            group: "premium".to_string(),
            recovery_at: None,
            message: Some("ActionRequiredError: ask admin".to_string()),
        }],
    };
    let json = serde_json::to_string(&q).expect("serialize");
    let back: QuotaJson = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(q, back);
    assert!(json.contains("partial"), "{json}");
    assert!(json.contains("premium"), "{json}");
}
