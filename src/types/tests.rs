// Tests for shared type parsing and display helpers.
// Exports: none; loaded by `types.rs` under `#[cfg(test)]`.
// Deps: super and chrono.

use super::*;
use chrono::Local;

fn sample_task(agent: AgentKind, custom_agent_name: Option<&str>) -> Task {
    Task {
        id: TaskId("t-test".to_string()),
        agent,
        custom_agent_name: custom_agent_name.map(|name| name.to_string()),
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Pending,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
    }
}

#[test]
fn agent_display_name_returns_custom_name() {
    let task = sample_task(AgentKind::Custom, Some("my-tool"));
    assert_eq!(task.agent_display_name(), "my-tool");
}

#[test]
fn agent_display_name_defaults_for_custom() {
    let task = sample_task(AgentKind::Custom, None);
    assert_eq!(task.agent_display_name(), "custom");
}

#[test]
fn agent_display_name_for_built_in_agents() {
    let task = sample_task(AgentKind::Codex, None);
    assert_eq!(task.agent_display_name(), "codex");
}

#[test]
fn memory_type_parse_str_roundtrip() {
    for memory_type in [
        MemoryType::Discovery,
        MemoryType::Convention,
        MemoryType::Lesson,
        MemoryType::Fact,
    ] {
        assert_eq!(MemoryType::parse_str(memory_type.as_str()), Some(memory_type));
    }
}

#[test]
fn memory_tier_parse_str_roundtrip() {
    for memory_tier in [
        MemoryTier::Identity,
        MemoryTier::Critical,
        MemoryTier::OnDemand,
        MemoryTier::Deep,
    ] {
        assert_eq!(MemoryTier::parse_str(memory_tier.as_str()), Some(memory_tier));
    }
}

#[test]
fn all_builtin_excludes_custom() {
    assert!(!AgentKind::ALL_BUILTIN.contains(&AgentKind::Custom));
}

#[test]
fn all_includes_custom() {
    assert!(AgentKind::ALL.contains(&AgentKind::Custom));
}

#[test]
fn all_builtin_matches_parse_str_coverage() {
    for kind in AgentKind::ALL_BUILTIN {
        assert_eq!(AgentKind::parse_str(kind.as_str()), Some(*kind));
    }
}

#[test]
fn pending_reason_parse_str_roundtrip() {
    for reason in [
        PendingReason::AgentStarting,
        PendingReason::RateLimited,
        PendingReason::WorkerCapacity,
        PendingReason::Unknown,
    ] {
        assert_eq!(PendingReason::parse_str(reason.as_str()), Some(reason));
    }
}

#[test]
fn profile_returns_some_for_all_builtin() {
    for kind in AgentKind::ALL_BUILTIN {
        assert!(kind.profile().is_some(), "{} should have a profile", kind.as_str());
    }
}

#[test]
fn profile_returns_none_for_custom() {
    assert!(AgentKind::Custom.profile().is_none());
}
