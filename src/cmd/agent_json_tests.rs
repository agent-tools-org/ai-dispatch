use std::collections::HashMap;
use crate::agent::custom::CustomAgentConfig;
use crate::cmd::agent_json_types::{
    AgentListJson, AgentJson, QuotaJson, ModelsJson, AvailableModelJson, HistoryJson, CategoryHistoryJson, LoadJson
};
use crate::types::AgentKind;
use super::{custom_has_endpoint, rate_limit_kind};

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
        },
        capabilities,
        models: ModelsJson {
            default: None,
            budget: Some("gpt-5.4-mini".to_string()),
            available: vec![AvailableModelJson {
                model: "gpt-5.5".to_string(),
                tier: "paid".to_string(),
                input_per_m: 1.25,
                output_per_m: 10.0,
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
fn delegate_opencode_without_endpoint_still_uses_opencode_rate_limit() {
    let mut config = sample_custom_config();
    config.base_url = None;
    assert!(!custom_has_endpoint(&config));
    assert_eq!(
        rate_limit_kind(AgentKind::Custom, Some(&config)),
        AgentKind::OpenCode
    );
}
