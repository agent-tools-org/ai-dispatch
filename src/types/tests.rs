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
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
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
        delivery_assessment: None,
    }
}

#[test]
fn agent_display_name_returns_custom_name() {
    let task = sample_task(AgentKind::Custom, Some("my-tool"));
    assert_eq!(task.agent_display_name(), "my-tool");
}

#[test]
fn generated_ids_use_32_bit_hex() {
    TaskId::set_generate_sequence_for_tests(&[]);
    assert_eq!(TaskId::generate().as_str().len(), 10);
    assert_eq!(WorkgroupId::generate().as_str().len(), 11);
}

#[test]
fn task_id_generate_can_be_seeded_for_collision_tests() {
    TaskId::set_generate_sequence_for_tests(&["t-collision", "t-retry"]);
    assert_eq!(TaskId::generate().as_str(), "t-collision");
    assert_eq!(TaskId::generate().as_str(), "t-retry");
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
fn display_route_is_cli_provider_model_and_keeps_unknown() {
    let task = sample_task(AgentKind::Codex, None);
    assert_eq!(task.display_route(), "codex/openai-chatgpt-plan/unknown");
    assert!(task.route().model.is_none());
}

#[test]
fn display_route_marks_inferred_attribution() {
    let mut task = sample_task(AgentKind::Codex, None);
    task.requested_model = Some("gpt-5.6".to_string());
    task.observed_model = Some("gpt-5.6".to_string());
    task.attribution_source = Some(AttributionSource::ConfirmedBySuccess);
    assert_eq!(
        task.display_route(),
        "codex/openai-chatgpt-plan/gpt-5.6 (inferred)"
    );
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
fn antigravity_alias_parses_to_agy_agent() {
    assert_eq!(AgentKind::parse_str("antigravity"), Some(AgentKind::Antigravity));
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
fn task_status_stalled_roundtrip() {
    assert_eq!(TaskStatus::parse_str("stalled"), Some(TaskStatus::Stalled));
    assert_eq!(TaskStatus::Stalled.as_str(), "stalled");
    assert_eq!(TaskStatus::Stalled.label(), "STALL");
    assert_eq!(serde_json::to_string(&TaskStatus::Stalled).unwrap(), "\"stalled\"");
    assert_eq!(
        serde_json::from_str::<TaskStatus>("\"stalled\"").unwrap(),
        TaskStatus::Stalled
    );
}

#[test]
fn task_status_transition_graph_matches_expected_edges() {
    let legal = [
        (TaskStatus::Waiting, TaskStatus::Pending),
        (TaskStatus::Waiting, TaskStatus::Running),
        (TaskStatus::Waiting, TaskStatus::Skipped),
        (TaskStatus::Waiting, TaskStatus::Failed),
        (TaskStatus::Waiting, TaskStatus::Stopped),
        (TaskStatus::Pending, TaskStatus::Running),
        (TaskStatus::Pending, TaskStatus::Skipped),
        (TaskStatus::Pending, TaskStatus::Failed),
        (TaskStatus::Pending, TaskStatus::Stopped),
        (TaskStatus::Running, TaskStatus::AwaitingInput),
        (TaskStatus::Running, TaskStatus::Stalled),
        (TaskStatus::Running, TaskStatus::Done),
        (TaskStatus::Running, TaskStatus::Failed),
        (TaskStatus::Running, TaskStatus::Stopped),
        (TaskStatus::AwaitingInput, TaskStatus::Running),
        (TaskStatus::AwaitingInput, TaskStatus::Stalled),
        (TaskStatus::AwaitingInput, TaskStatus::Done),
        (TaskStatus::AwaitingInput, TaskStatus::Failed),
        (TaskStatus::AwaitingInput, TaskStatus::Stopped),
        (TaskStatus::Stalled, TaskStatus::Running),
        (TaskStatus::Stalled, TaskStatus::Done),
        (TaskStatus::Stalled, TaskStatus::Failed),
        (TaskStatus::Stalled, TaskStatus::Stopped),
        (TaskStatus::Done, TaskStatus::Merged),
        (TaskStatus::Done, TaskStatus::Failed),
        (TaskStatus::Failed, TaskStatus::Merged),
        (TaskStatus::Stopped, TaskStatus::Merged),
    ];
    for current in TaskStatus::ALL {
        for next in TaskStatus::ALL {
            let expected = current == next || legal.contains(&(current, next));
            assert_eq!(
                current.can_transition_to(next),
                expected,
                "{} -> {}",
                current.as_str(),
                next.as_str()
            );
        }
    }
}

#[test]
fn failed_to_done_is_only_a_rescue_transition() {
    assert!(!TaskStatus::Failed.can_transition_to(TaskStatus::Done));
    assert!(TaskStatus::Failed.can_rescue_to_done(TaskStatus::Done));
    assert!(!TaskStatus::Stopped.can_rescue_to_done(TaskStatus::Done));
}

#[test]
fn message_direction_roundtrip() {
    for direction in [MessageDirection::In, MessageDirection::Out] {
        assert_eq!(MessageDirection::try_from(direction.as_str()).ok(), Some(direction));
    }
}

#[test]
fn message_source_roundtrip() {
    for source in [
        MessageSource::Reply,
        MessageSource::Steer,
        MessageSource::UnstickAuto,
        MessageSource::AgentAck,
    ] {
        assert_eq!(MessageSource::try_from(source.as_str()).ok(), Some(source));
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

#[test]
fn task_delivery_assessment_maps_empty_diff() {
    let mut task = sample_task(AgentKind::Codex, None);
    task.delivery_assessment = Some(DeliveryAssessment::EmptyDiff);
    assert_eq!(task.delivery_assessment(), Some(DeliveryAssessment::EmptyDiff));
}

#[test]
fn task_delivery_assessment_maps_hollow_output() {
    let mut task = sample_task(AgentKind::Codex, None);
    task.delivery_assessment = Some(DeliveryAssessment::HollowOutput);
    assert_eq!(
        task.delivery_assessment(),
        Some(DeliveryAssessment::HollowOutput)
    );
}

#[test]
fn resumable_agents_report_session_support() {
    for kind in [
        AgentKind::OpenCode,
        AgentKind::CommandCode,
        AgentKind::Kilo,
        AgentKind::MiMoCode,
        AgentKind::Droid,
        AgentKind::Codex,
        AgentKind::Qwen,
    ] {
        assert!(kind.supports_session_resume(), "{kind} should resume sessions");
    }
    for kind in [AgentKind::Gemini, AgentKind::Cursor, AgentKind::Claude, AgentKind::Custom] {
        assert!(!kind.supports_session_resume(), "{kind} should not resume sessions");
    }
}


#[test]
fn event_kind_parse_or_warn_falls_back_and_warns_once() {
    assert_eq!(EventKind::parse_or_warn("tool_call"), EventKind::ToolCall);
    assert_eq!(EventKind::parse_or_warn("bogus_kind"), EventKind::Reasoning);
    // Dedup: the first sighting of an unknown kind warns, repeats stay silent.
    assert!(super::status::note_unknown_event_kind("bogus_kind_dedup"));
    assert!(!super::status::note_unknown_event_kind("bogus_kind_dedup"));
}

#[test]
fn task_event_full_detail_prefers_metadata_full() {
    let mut event = TaskEvent {
        task_id: TaskId("t-full".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Reasoning,
        detail: "truncated...".to_string(),
        metadata: Some(serde_json::json!({ "full": "the complete untruncated text" })),
    };
    assert_eq!(event.full_detail(), "the complete untruncated text");
    event.metadata = None;
    assert_eq!(event.full_detail(), "truncated...");
}
